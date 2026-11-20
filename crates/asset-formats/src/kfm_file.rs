// kfm_file.rs — Gamebryo Keyframe Manager (.kfm, .kfm_wii) format parser

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KfmSequence {
    pub sequence_id: u32,
    pub filename: String,
    pub sequence_name: String,
    pub anim_index: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KfmTransition {
    pub src_id: u32,
    pub des_id: u32,
    pub trans_type: u32,
    pub duration: f32,
    #[serde(default)]
    pub blend_pairs: Vec<KfmBlendPair>,
    #[serde(default)]
    pub chain: Vec<KfmChainEntry>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KfmBlendPair {
    pub start_key: String,
    pub target_key: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KfmChainEntry {
    pub sequence_id: u32,
    pub duration: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KfmSequenceGroup {
    pub group_id: u32,
    pub name: String,
    #[serde(default)]
    pub sequences: Vec<KfmGroupSequence>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KfmGroupSequence {
    pub sequence_id: u32,
    pub priority: i32,
    pub weight: f32,
    pub ease_in_time: f32,
    pub ease_out_time: f32,
    pub synchronize_sequence_id: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KfmFile {
    pub version: String,
    pub model_path: String,
    pub model_root: String,
    /// Transition policy used when an animation is synchronized.
    #[serde(default)]
    pub sync_type: u32,
    /// Transition policy used when an animation is not synchronized.
    #[serde(default)]
    pub nonsync_type: u32,
    /// Default synchronized transition duration in seconds.
    #[serde(default)]
    pub sync_duration: f32,
    /// Default non-synchronized transition duration in seconds.
    #[serde(default)]
    pub nonsync_duration: f32,
    pub sequences: Vec<KfmSequence>,
    pub transitions: Vec<KfmTransition>,
    pub groups: Vec<KfmSequenceGroup>,
    /// True when the source used the binary KFM container.
    #[serde(default)]
    pub source_is_binary: bool,
    /// Binary byte order, retained so a binary Wii file can be rewritten in
    /// its original order. ASCII files always use the text serializer.
    #[serde(default)]
    pub source_is_big_endian: bool,
}

impl KfmFile {
    pub fn load_bytes(data: &[u8]) -> Result<Self, String> {
        let content_str = String::from_utf8_lossy(data);
        let first_line = content_str.lines().next().unwrap_or("");
        if first_line.ends_with('b') && first_line.starts_with(";Gamebryo KFM") {
            Self::parse_binary(data)
        } else if content_str.starts_with(";Gamebryo KFM File") {
            Self::parse_ascii(&content_str)
        } else {
            Self::parse_binary(data)
        }
    }

    pub fn parse_ascii(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        let first_line = lines.next().ok_or("Empty KFM file")?.trim();
        let version = first_line
            .trim_start_matches(";Gamebryo KFM File Version ")
            .to_string();

        let mut model_path = String::new();
        let mut model_root = String::new();
        let mut sequences = Vec::new();

        for line in lines {
            let line_str = line.trim();
            if line_str.is_empty() || line_str.starts_with(';') {
                continue;
            }

            let parts: Vec<&str> = line_str.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            if model_path.is_empty()
                && (parts[0].contains(".nif")
                    || parts[0].contains("nif_wii")
                    || parts[0].contains('.'))
            {
                model_path = parts[0].to_string();
                if parts.len() > 1 {
                    model_root = parts[1].to_string();
                }
            } else {
                let kf_path = parts[0].to_string();
                let seq_name = if parts.len() > 1 {
                    parts[1].to_string()
                } else {
                    format!("Seq_{}", sequences.len())
                };
                let seq_id = parts
                    .get(3)
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(sequences.len() as u32);
                let anim_index = parts
                    .get(2)
                    .and_then(|value| value.parse::<i32>().ok())
                    .unwrap_or(0);
                sequences.push(KfmSequence {
                    sequence_id: seq_id,
                    filename: kf_path,
                    sequence_name: seq_name,
                    anim_index,
                });
            }
        }

        Ok(KfmFile {
            version,
            model_path,
            model_root,
            sync_type: 0,
            nonsync_type: 0,
            sync_duration: 0.0,
            nonsync_duration: 0.0,
            sequences,
            transitions: Vec::new(),
            groups: Vec::new(),
            source_is_binary: false,
            source_is_big_endian: false,
        })
    }

    pub fn parse_binary(data: &[u8]) -> Result<Self, String> {
        let mut cursor = std::io::Cursor::new(data);
        use std::io::Read;

        let mut header_buf = Vec::new();
        let mut b = [0u8; 1];
        while cursor.read_exact(&mut b).is_ok() {
            if b[0] == b'\n' {
                break;
            }
            header_buf.push(b[0]);
        }
        let first_line = String::from_utf8_lossy(&header_buf).trim().to_string();
        let version = first_line
            .trim_start_matches(";Gamebryo KFM File Version ")
            .trim_end_matches('b')
            .to_string();

        let mut endian_buf = [0u8; 1];
        if cursor.read_exact(&mut endian_buf).is_err() {
            return Err("Unexpected EOF reading endian flag".into());
        }
        let is_be = endian_buf[0] == 0;

        let read_u32 = |c: &mut std::io::Cursor<&[u8]>| -> Result<u32, String> {
            let mut buf = [0u8; 4];
            c.read_exact(&mut buf).map_err(|e| e.to_string())?;
            Ok(if is_be {
                u32::from_be_bytes(buf)
            } else {
                u32::from_le_bytes(buf)
            })
        };

        let read_f32 = |c: &mut std::io::Cursor<&[u8]>| -> Result<f32, String> {
            let mut buf = [0u8; 4];
            c.read_exact(&mut buf).map_err(|e| e.to_string())?;
            Ok(if is_be {
                f32::from_be_bytes(buf)
            } else {
                f32::from_le_bytes(buf)
            })
        };

        let read_str = |c: &mut std::io::Cursor<&[u8]>| -> Result<String, String> {
            let len = read_u32(c)? as usize;
            if len == 0 {
                return Ok(String::new());
            }
            if len > data.len().saturating_sub(c.position() as usize) {
                return Err("Truncated KFM string".into());
            }
            let mut buf = vec![0u8; len];
            c.read_exact(&mut buf).map_err(|e| e.to_string())?;
            Ok(String::from_utf8_lossy(&buf).to_string())
        };

        let model_path = read_str(&mut cursor)?;
        let model_root = read_str(&mut cursor)?;

        let read_fixed = |c: &mut std::io::Cursor<&[u8]>| -> Result<String, String> {
            // EM1 DOL803f65e4: signed length <=0 denotes a null fixed string.
            let length = read_u32(c)? as i32;
            if length <= 0 { return Ok(String::new()); }
            let start = c.position() as usize;
            let end = start.checked_add(length as usize).ok_or("KFM string overflow")?;
            let bytes = data.get(start..end).ok_or("Truncated KFM fixed string")?;
            c.set_position(end as u64);
            String::from_utf8(bytes.to_vec()).map_err(|e| format!("Invalid KFM fixed string: {e}"))
        };
        let sync_type = read_u32(&mut cursor)?;
        let nonsync_type = read_u32(&mut cursor)?;
        let sync_duration = read_f32(&mut cursor)?;
        let nonsync_duration = read_f32(&mut cursor)?;

        let num_seqs = read_u32(&mut cursor)? as usize;
        let mut sequences = Vec::new();
        let mut transitions = Vec::new();

        for _ in 0..num_seqs {
            let sequence_id = read_u32(&mut cursor)?;
            let filename = read_str(&mut cursor)?;
            let sequence_name = read_str(&mut cursor)?;

            sequences.push(KfmSequence {
                sequence_id,
                filename,
                sequence_name,
                anim_index: 0,
            });

            let num_trans = read_u32(&mut cursor)?;
            {
                for _ in 0..num_trans {
                    let des_id = read_u32(&mut cursor)?;
                    let trans_type = read_u32(&mut cursor)?;
                    let duration = if trans_type != 4 && trans_type != 5 {
                        read_f32(&mut cursor)?
                    } else {
                        0.0
                    };
                    let mut blend_pairs = Vec::new();
                    let mut chain = Vec::new();
                    if trans_type != 4 && trans_type != 5 {
                        // EM1 DOL803f4e64..4fc0: these counts are present even
                        // when empty, for every non-default transition type.
                        for _ in 0..read_u32(&mut cursor)? {
                            blend_pairs.push(KfmBlendPair {
                                start_key: read_fixed(&mut cursor)?,
                                target_key: read_fixed(&mut cursor)?,
                            });
                        }
                        for _ in 0..read_u32(&mut cursor)? {
                            chain.push(KfmChainEntry {
                                sequence_id: read_u32(&mut cursor)?,
                                duration: read_f32(&mut cursor)?,
                            });
                        }
                    }
                    transitions.push(KfmTransition {
                        src_id: sequence_id,
                        des_id,
                        trans_type,
                        duration,
                        blend_pairs,
                        chain,
                    });
                }
            }
        }

        // EM1 DOL803f529c..53c4: groups follow the sequence map, including
        // a mandatory zero count when no groups were authored.
        let mut groups = Vec::new();
        for _ in 0..read_u32(&mut cursor)? {
            let group_id = read_u32(&mut cursor)?;
            let name = read_fixed(&mut cursor)?;
            let mut members = Vec::new();
            for _ in 0..read_u32(&mut cursor)? {
                members.push(KfmGroupSequence {
                    sequence_id: read_u32(&mut cursor)?,
                    priority: read_u32(&mut cursor)? as i32,
                    weight: read_f32(&mut cursor)?,
                    ease_in_time: read_f32(&mut cursor)?,
                    ease_out_time: read_f32(&mut cursor)?,
                    synchronize_sequence_id: read_u32(&mut cursor)?,
                });
            }
            groups.push(KfmSequenceGroup { group_id, name, sequences: members });
        }

        Ok(KfmFile {
            version,
            model_path,
            model_root,
            sync_type,
            nonsync_type,
            sync_duration,
            nonsync_duration,
            sequences,
            transitions,
            groups,
            source_is_binary: true,
            source_is_big_endian: is_be,
        })
    }

    /// Serialize the understood KFM fields while retaining the source
    /// container kind and byte order.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        if !self.source_is_binary {
            let mut text = format!(
                ";Gamebryo KFM File Version {}\n{}\t{}\n",
                self.version, self.model_path, self.model_root
            );
            for sequence in &self.sequences {
                text.push_str(&sequence.filename);
                text.push('\t');
                text.push_str(&sequence.sequence_name);
                text.push('\t');
                text.push_str(&sequence.anim_index.to_string());
                text.push('\t');
                text.push_str(&sequence.sequence_id.to_string());
                text.push('\n');
            }
            return Ok(text.into_bytes());
        }

        fn write_u32(out: &mut Vec<u8>, value: u32, big_endian: bool) {
            out.extend(if big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            });
        }
        fn write_f32(out: &mut Vec<u8>, value: f32, big_endian: bool) {
            out.extend(if big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            });
        }
        fn write_str(out: &mut Vec<u8>, value: &str, big_endian: bool) -> Result<(), String> {
            let bytes = value.as_bytes();
            let length = u32::try_from(bytes.len()).map_err(|_| "KFM string is too long")?;
            write_u32(out, length, big_endian);
            out.extend(bytes);
            Ok(())
        }
        let big_endian = self.source_is_big_endian;
        let mut out = format!(";Gamebryo KFM File Version {}b\n", self.version).into_bytes();
        out.push(if big_endian { 0 } else { 1 });
        write_str(&mut out, &self.model_path, big_endian)?;
        write_str(&mut out, &self.model_root, big_endian)?;
        write_u32(&mut out, self.sync_type, big_endian);
        write_u32(&mut out, self.nonsync_type, big_endian);
        write_f32(&mut out, self.sync_duration, big_endian);
        write_f32(&mut out, self.nonsync_duration, big_endian);
        write_u32(
            &mut out,
            u32::try_from(self.sequences.len()).map_err(|_| "Too many KFM sequences")?,
            big_endian,
        );
        for sequence in &self.sequences {
            write_u32(&mut out, sequence.sequence_id, big_endian);
            write_str(&mut out, &sequence.filename, big_endian)?;
            write_str(&mut out, &sequence.sequence_name, big_endian)?;
            let outgoing = self
                .transitions
                .iter()
                .filter(|transition| transition.src_id == sequence.sequence_id)
                .collect::<Vec<_>>();
            write_u32(
                &mut out,
                u32::try_from(outgoing.len()).map_err(|_| "Too many KFM transitions")?,
                big_endian,
            );
            for transition in outgoing {
                write_u32(&mut out, transition.des_id, big_endian);
                write_u32(&mut out, transition.trans_type, big_endian);
                if transition.trans_type != 4 && transition.trans_type != 5 {
                    write_f32(&mut out, transition.duration, big_endian);
                    write_u32(&mut out, u32::try_from(transition.blend_pairs.len())
                        .map_err(|_| "Too many KFM blend pairs")?, big_endian);
                    for pair in &transition.blend_pairs {
                        write_str(&mut out, &pair.start_key, big_endian)?;
                        write_str(&mut out, &pair.target_key, big_endian)?;
                    }
                    write_u32(&mut out, u32::try_from(transition.chain.len())
                        .map_err(|_| "Too many KFM chain entries")?, big_endian);
                    for entry in &transition.chain {
                        write_u32(&mut out, entry.sequence_id, big_endian);
                        write_f32(&mut out, entry.duration, big_endian);
                    }
                }
            }
        }
        write_u32(&mut out, u32::try_from(self.groups.len())
            .map_err(|_| "Too many KFM sequence groups")?, big_endian);
        for group in &self.groups {
            write_u32(&mut out, group.group_id, big_endian);
            write_str(&mut out, &group.name, big_endian)?;
            write_u32(&mut out, u32::try_from(group.sequences.len())
                .map_err(|_| "Too many KFM group members")?, big_endian);
            for member in &group.sequences {
                write_u32(&mut out, member.sequence_id, big_endian);
                write_u32(&mut out, member.priority as u32, big_endian);
                write_f32(&mut out, member.weight, big_endian);
                write_f32(&mut out, member.ease_in_time, big_endian);
                write_f32(&mut out, member.ease_out_time, big_endian);
                write_u32(&mut out, member.synchronize_sequence_id, big_endian);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_transition_payload_keeps_following_sequence_aligned() {
        // Independently assembled modern binary layout, not generated by
        // to_bytes. Explicit transitions always carry both array counts.
        for big_endian in [false, true] {
            let mut bytes = b";Gamebryo KFM File Version 2.5.0.0b\n".to_vec();
            bytes.push(if big_endian { 0 } else { 1 });
            let word = |out: &mut Vec<u8>, value: u32| {
                out.extend(if big_endian { value.to_be_bytes() } else { value.to_le_bytes() });
            };
            let string = |out: &mut Vec<u8>, value: &str| {
                word(out, value.len() as u32); out.extend(value.as_bytes());
            };
            string(&mut bytes, "model.nif"); string(&mut bytes, "Root");
            for value in [1, 2, 0, 0, 2] { word(&mut bytes, value); }
            word(&mut bytes, 10); string(&mut bytes, "motion.kf"); string(&mut bytes, "Rest");
            for value in [1, 20, 3, 0.5f32.to_bits(), 1] { word(&mut bytes, value); }
            string(&mut bytes, "end"); string(&mut bytes, "start");
            for value in [1, 30, 0.75f32.to_bits()] { word(&mut bytes, value); }
            let second_sequence = bytes.len();
            word(&mut bytes, 20); string(&mut bytes, "motion.kf"); string(&mut bytes, "Spawn");
            word(&mut bytes, 0);
            word(&mut bytes, 2); // Two groups, second one empty.
            word(&mut bytes, 7); string(&mut bytes, "Together"); word(&mut bytes, 2);
            for value in [10, (-3i32) as u32, 0.75f32.to_bits(), 0.125f32.to_bits(),
                0.25f32.to_bits(), 20, 20, 4, 1.0f32.to_bits(), 0, 0, 0xffff_fffe] {
                word(&mut bytes, value);
            }
            let empty_group = bytes.len();
            word(&mut bytes, 8); string(&mut bytes, "Empty"); word(&mut bytes, 0);
            let parsed = KfmFile::load_bytes(&bytes).unwrap();
            assert_eq!(parsed.sequences[1].sequence_id, 20);
            assert_eq!(parsed.sequences[1].sequence_name, "Spawn");
            let transition = &parsed.transitions[0];
            assert_eq!(transition.blend_pairs[0].start_key, "end");
            assert_eq!(transition.blend_pairs[0].target_key, "start");
            assert_eq!(transition.chain[0].sequence_id, 30);
            assert_eq!(transition.chain[0].duration, 0.75);
            assert_eq!(parsed.groups.len(), 2);
            assert_eq!(parsed.groups[0].group_id, 7);
            assert_eq!(parsed.groups[0].name, "Together");
            let first = &parsed.groups[0].sequences[0];
            assert_eq!(first.sequence_id, 10);
            assert_eq!(first.priority, -3);
            assert_eq!(first.weight, 0.75);
            assert_eq!(first.ease_in_time, 0.125);
            assert_eq!(first.ease_out_time, 0.25);
            assert_eq!(first.synchronize_sequence_id, 20);
            assert_eq!(parsed.groups[0].sequences[1].synchronize_sequence_id, 0xffff_fffe);
            assert!(parsed.groups[1].sequences.is_empty());
            assert_eq!(parsed.to_bytes().unwrap(), bytes);
            // Truncating chain payload must not be accepted as an empty list.
            assert!(KfmFile::load_bytes(&bytes[..second_sequence - 1]).is_err());
            assert!(KfmFile::load_bytes(&bytes[..empty_group - 1]).is_err());
            assert!(KfmFile::load_bytes(&bytes[..bytes.len() - 1]).is_err());
        }
    }

    #[test]
    fn test_load_sample_kfm_wii() {
        let path = r"D:\Modding\Assets\EM2P6\Environments\_Test\Jason\StreamTest01\EnvironmentObjects.kfm_wii";
        if std::path::Path::new(path).exists() {
            let data = std::fs::read(path).unwrap();
            let kfm = KfmFile::load_bytes(&data).unwrap();
            assert!(!kfm.model_path.is_empty());
            assert!(!kfm.sequences.is_empty());
            assert_eq!(kfm.sequences[0].filename, r".\EnvironmentObjects.kf_wii");
        }
    }

    #[test]
    fn binary_serializer_roundtrips_editable_sequence_fields() {
        let source = KfmFile {
            version: "4.0.0.0".into(),
            model_path: "Actors/Mickey.nif_wii".into(),
            model_root: "Root".into(),
            sync_type: 3,
            nonsync_type: 5,
            sync_duration: 0.5,
            nonsync_duration: 1.25,
            sequences: vec![KfmSequence {
                sequence_id: 42,
                filename: "Actors/Mickey_Idle.kf_wii".into(),
                sequence_name: "Idle".into(),
                anim_index: 0,
            }],
            transitions: vec![KfmTransition {
                src_id: 42,
                des_id: 42,
                trans_type: 2,
                duration: 0.25,
                ..Default::default()
            }],
            groups: Vec::new(),
            source_is_binary: true,
            source_is_big_endian: true,
        };
        let bytes = source.to_bytes().unwrap();
        let parsed = KfmFile::load_bytes(&bytes).unwrap();
        assert!(parsed.source_is_binary);
        assert!(parsed.source_is_big_endian);
        assert_eq!(parsed.model_path, source.model_path);
        assert_eq!(parsed.sync_type, source.sync_type);
        assert_eq!(parsed.nonsync_type, source.nonsync_type);
        assert_eq!(parsed.sync_duration, source.sync_duration);
        assert_eq!(parsed.nonsync_duration, source.nonsync_duration);
        assert_eq!(parsed.sequences[0].sequence_id, 42);
        assert_eq!(parsed.sequences[0].filename, source.sequences[0].filename);
        assert_eq!(parsed.sequences[0].sequence_name, "Idle");
        assert_eq!(parsed.transitions.len(), 1);
        assert_eq!(parsed.transitions[0].src_id, 42);
        assert_eq!(parsed.transitions[0].duration, 0.25);
        assert_eq!(parsed.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn little_endian_roundtrip_preserves_multiple_transitions_and_implicit_duration() {
        let source = KfmFile {
            version: "4.0.0.0".into(),
            model_path: "Actors/Mickey.nif_wii".into(),
            model_root: "Root".into(),
            sync_type: 1,
            nonsync_type: 2,
            sync_duration: 0.125,
            nonsync_duration: 0.75,
            sequences: vec![
                KfmSequence {
                    sequence_id: 10,
                    filename: "Idle.kf_wii".into(),
                    sequence_name: "Idle".into(),
                    anim_index: 0,
                },
                KfmSequence {
                    sequence_id: 20,
                    filename: "Walk.kf_wii".into(),
                    sequence_name: "Walk".into(),
                    anim_index: 1,
                },
            ],
            transitions: vec![
                KfmTransition {
                    src_id: 10,
                    des_id: 20,
                    trans_type: 4,
                    duration: 99.0,
                    ..Default::default()
                },
                KfmTransition {
                    src_id: 20,
                    des_id: 10,
                    trans_type: 3,
                    duration: 0.5,
                    ..Default::default()
                },
            ],
            groups: Vec::new(),
            source_is_binary: true,
            source_is_big_endian: false,
        };
        let bytes = source.to_bytes().unwrap();
        let parsed = KfmFile::load_bytes(&bytes).unwrap();
        assert!(parsed.source_is_binary);
        assert!(!parsed.source_is_big_endian);
        assert_eq!(parsed.sequences.len(), 2);
        assert_eq!(parsed.transitions.len(), 2);
        assert_eq!(parsed.transitions[0].trans_type, 4);
        assert_eq!(parsed.transitions[0].duration, 0.0);
        assert_eq!(parsed.transitions[1].src_id, 20);
        assert_eq!(parsed.transitions[1].duration, 0.5);
        assert_eq!(parsed.to_bytes().unwrap(), bytes);
    }
}
