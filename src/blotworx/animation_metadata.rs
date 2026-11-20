//! Authored float-slot metadata for the verified 32-bit Wii and PC layouts.
use anyhow::{ensure, Context, Result};
use epic_mickey_lib_rs::hkx_file::HkxFile;
/// hkaAnimation::m_extractedMotion at +0x18, reached through the authored
/// hkaAnimationBinding (+0x0c). Null is meaningful: the game returns identity.
pub fn has_extracted_motion(file: &HkxFile) -> Result<bool> {
    let view = View::new(file)?;
    let animation = if file.root_class_name=="hkaSplineCompressedAnimation" {
        ensure!(file.header.contents_section_index as usize==view.section,"Standalone animation root is outside data section");
        let root=view.object("hkaSplineCompressedAnimation")?;
        ensure!(file.header.contents_section_offset as usize==root,"Standalone animation root differs from its class fixup");root
    } else {
    let binding = view.object("hkaAnimationBinding")?;
    if let Some(pointer) = file
        .global_fixups
        .get(view.section)
        .context("Missing global fixups")?
        .iter()
        .find(|f| f.src_offset as usize == binding + 12)
    {
        ensure!(
            pointer.dst_section as usize == view.section,
            "Cross-section animation binding is not implemented"
        );
        pointer.dst_offset as usize
    } else {
        view.pointer(binding + 12)?
    }
    };
    let field = animation
        .checked_add(24)
        .context("Animation pointer overflow")?;
    let pointer = view
        .file
        .local_fixups
        .get(view.section)
        .context("Missing local fixups")?
        .iter()
        .find(|f| f.src_offset as usize == field);
    if let Some(pointer) = pointer {
        ensure!(
            (pointer.dst_offset as usize) < view.data.len(),
            "Invalid extracted motion reference"
        );
        return Ok(true);
    }
    // Global references must not accidentally be classified as null just
    // because their on-disk pointer word is zero before relocation.
    if view
        .file
        .global_fixups
        .get(view.section)
        .context("Missing global fixups")?
        .iter()
        .any(|f| f.src_offset as usize == field)
    {
        return Ok(true);
    }
    ensure!(
        view.word(field)? == 0,
        "Unrelocated non-null extracted motion pointer"
    );
    Ok(false)
}
pub fn float_slots(file: &HkxFile) -> Result<Vec<String>> {
    let view = View::new(file)?;
    let skeleton = view.object("hkaSkeleton")?;
    let count = view.word(skeleton + 52)? as usize;
    if count == 0 {
        return Ok(vec![]);
    }
    let array = view.pointer(skeleton + 48)?;
    ensure!(
        count <= view.data.len().saturating_sub(array) / 4,
        "Truncated float slot array"
    );
    (0..count)
        .map(|i| {
            let start = view.pointer(array + i * 4)?;
            let tail = view
                .data
                .get(start..)
                .context("Invalid slot name pointer")?;
            let end = tail
                .iter()
                .position(|&b| b == 0)
                .context("Unterminated slot name")?;
            Ok(std::str::from_utf8(&tail[..end])?.to_owned())
        })
        .collect()
}
pub fn float_binding(file: &HkxFile) -> Result<Vec<i16>> {
    let view = View::new(file)?;
    let binding = view.object("hkaAnimationBinding")?;
    let count = view.word(binding + 32)? as usize;
    if count == 0 {
        return Ok(vec![]);
    }
    let start = view.pointer(binding + 28)?;
    ensure!(
        count <= view.data.len().saturating_sub(start) / 2,
        "Truncated float binding"
    );
    (0..count)
        .map(|i| {
            let bytes = view.data[start + i * 2..start + i * 2 + 2].try_into()?;
            Ok(if file.header.endianness == 1 {
                i16::from_le_bytes(bytes)
            } else {
                i16::from_be_bytes(bytes)
            })
        })
        .collect()
}
pub(super) struct View<'a> {
    pub(super) file: &'a HkxFile,
    pub(super) section: usize,
    pub(super) data: &'a [u8],
}
impl<'a> View<'a> {
    pub(super) fn new(file: &'a HkxFile) -> Result<Self> {
        ensure!(
            file.header.pointer_size == 4
                && ((file.header.endianness == 0
                    && file.header.contents_version.starts_with("Havok-7.0"))
                    || (file.header.endianness == 1
                        && file.header.contents_version == "hk_2012.1.0-r1")),
            "Unsupported float metadata ABI"
        );
        let section = file
            .sections
            .iter()
            .position(|s| s.section_tag == "__data__")
            .context("Missing data section")?;
        Ok(Self {
            file,
            section,
            data: file
                .section_data
                .get(section)
                .context("Missing section bytes")?,
        })
    }
    pub(super) fn word(&self, at: usize) -> Result<u32> {
        let bytes = self
            .data
            .get(at..at + 4)
            .context("Truncated float metadata")?
            .try_into()?;
        Ok(if self.file.header.endianness == 1 {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    }
    pub(super) fn pointer(&self, at: usize) -> Result<usize> {
        self.file
            .local_fixups
            .get(self.section)
            .context("Missing fixups")?
            .iter()
            .find(|f| f.src_offset as usize == at)
            .map(|f| f.dst_offset as usize)
            .context("Missing float metadata pointer")
    }
    pub(super) fn object(&self, name: &str) -> Result<usize> {
        let mut matches = vec![];
        for f in self
            .file
            .virtual_fixups
            .get(self.section)
            .context("Missing class fixups")?
        {
            let bytes = self
                .file
                .section_data
                .get(f.section_index as usize)
                .and_then(|s| s.get(f.class_name_offset as usize..))
                .context("Invalid class name pointer")?;
            if bytes.split(|&b| b == 0).next() == Some(name.as_bytes()) {
                matches.push(f.src_offset as usize);
            }
        }
        ensure!(
            matches.len() == 1,
            "Expected exactly one {name}, got {}",
            matches.len()
        );
        Ok(matches[0])
    }
}
