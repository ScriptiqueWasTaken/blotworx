//! WRLD1 scene request list, REL21740..218dc. Live construction is owned by
//! scene_resources from shared/part records and the filtered manager queries.
//! Do not substitute every imported scene or entity/component persistence.
use anyhow::{ensure, Context, Result};
pub fn decode(payload: &[u8]) -> Result<Vec<String>> {
    let records = super::save_game::records(payload)?;
    let count = records
        .iter()
        .find(|record| record.name == "COUNT")
        .context("World save count missing")?
        .data;
    ensure!(count.len() >= 4, "Truncated world save count");
    let count = u32::from_be_bytes(count[..4].try_into()?) as usize;
    // Every declared path needs its own named record; reject impossible counts
    // before allocating, rather than impose a made-up scene limit.
    ensure!(
        count < records.len(),
        "World save count exceeds available scene records"
    );
    let mut scenes = Vec::with_capacity(count);
    for index in 0..count {
        let name = index.to_string();
        let data = records
            .iter()
            .find(|record| record.name == name)
            .context("World save scene record missing")?
            .data;
        let end = data
            .iter()
            .position(|&byte| byte == 0)
            .context("Unterminated world save scene")?;
        scenes.push(
            std::str::from_utf8(&data[..end])
                .context("Unsupported world scene name encoding")?
                .to_owned(),
        );
    }
    Ok(scenes)
}
/// The caller must supply the original ordered request list, not a set of
/// imported assets. Section final padding belongs to MemoryWriter/EndSection.
pub fn encode(scenes: &[String]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    super::save_game::append_record(
        &mut output,
        "count",
        &u32::try_from(scenes.len())?.to_be_bytes(),
        4,
    )?;
    for (index, scene) in scenes.iter().enumerate() {
        ensure!(!scene.contains('\0'), "World scene contains embedded NUL");
        let mut data = scene.as_bytes().to_vec();
        data.push(0);
        super::save_game::append_record(&mut output, &index.to_string(), &data, 1)?;
    }
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_scene_strings_preserve_case_duplicates_and_unaligned_lengths() -> Result<()> {
        for scenes in [
            vec![],
            vec!["a.gsa".to_owned(), "Scene/ABC.gsa".into(), "a.gsa".into()],
        ] {
            let payload = encode(&scenes)?;
            assert_eq!(decode(&payload)?, scenes);
            let mut padded = payload.clone();
            padded.resize(padded.len().div_ceil(4) * 4, 0);
            assert_eq!(decode(&padded)?, scenes);
        }
        let mut payload = encode(&["abc".into()])?;
        payload[12..16].copy_from_slice(&2u32.to_be_bytes());
        assert!(decode(&payload).is_err());
        assert!(decode(&[]).is_err());
        assert!(encode(&["abc\0def".into()]).is_err());
        Ok(())
    }
    #[test]
    #[ignore = "requires original populated save in BLOTWORX_TEST_SAVE"]
    fn original_world_scene_section_reencodes_byte_exact() -> Result<()> {
        let path = std::env::var_os("BLOTWORX_TEST_SAVE").context("Set BLOTWORX_TEST_SAVE")?;
        let bytes = std::fs::read(&path)?;
        let archive = super::super::save_game::Archive::parse(&bytes)?;
        let section = archive.section(*b"WRLD")?;
        assert_eq!(section.version, 1);
        let scenes = decode(&section.payload)?;
        assert!(!scenes.is_empty());
        let mut encoded = encode(&scenes)?;
        encoded.resize(encoded.len().div_ceil(4) * 4, 0);
        assert_eq!(&encoded, &*section.payload);
        assert_eq!(std::fs::read(path)?, bytes);
        Ok(())
    }
}
