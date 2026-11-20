//! Preserve the complete serialized hkaSkeleton reference Qs records. The
//! renderer's HkxBone projection intentionally exposes only XYZ; attachments
//! must not replace original padding/auxiliary lanes with invented zeros.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;
pub fn read(file: &HkxFile, count: usize) -> Result<Vec<super::actor_bone_query::Pose>> {
    let data = file
        .sections
        .iter()
        .position(|s| s.section_tag == "__data__")
        .context("Rig data section missing")?;
    let bytes = &file.section_data[data];
    let mut objects = file.virtual_fixups[data].iter().filter(|fixup| {
        file.section_data
            .get(fixup.section_index as usize)
            .and_then(|names| names.get(fixup.class_name_offset as usize..))
            .is_some_and(|name| name.starts_with(b"hkaSkeleton\0"))
    });
    let object = objects.next().context("Rig skeleton record missing")?;
    ensure!(
        objects.next().is_none(),
        "Multiple rig skeletons need container/interface selection"
    );
    let at = object.src_offset as usize;
    let word = |offset| -> Result<u32> {
        let b = bytes
            .get(offset..offset + 4)
            .context("Truncated reference pose record")?
            .try_into()?;
        Ok(if file.header.endianness == 1 {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    ensure!(
        word(at + 40)? as usize == count,
        "Native rig pose count differs from skeleton"
    );
    let start = file.local_fixups[data]
        .iter()
        .find(|f| f.src_offset as usize == at + 36)
        .context("Rig reference pose pointer missing")?
        .dst_offset as usize;
    ensure!(
        count <= bytes.len().saturating_sub(start) / 48,
        "Truncated rig reference pose array"
    );
    (0..count)
        .map(|bone| {
            let mut pose = [0.; 12];
            for (lane, value) in pose.iter_mut().enumerate() {
                *value = f32::from_bits(word(start + bone * 48 + lane * 4)?);
            }
            ensure!(
                pose.iter().all(|value| value.is_finite()),
                "Nonfinite native rig pose"
            );
            Ok(pose)
        })
        .collect()
}
