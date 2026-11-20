//! Borrowed Wwise bank containers. Node/action semantics remain versioned.
//! Wii version46 uses BE words and u32 HIRC kinds; PC version72 uses LE words
//! and u8 HIRC kinds. Never feed modern payloads to the legacy node decoder.
use anyhow::{ensure, Context, Result};


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format { Wii46, Pc72 }
impl Format {
    pub fn word(self, bytes: &[u8], at: usize) -> Result<u32> {
        let end = at.checked_add(4).context("Bank word offset overflow")?;
        let raw = bytes.get(at..end).context("Truncated bank word")?.try_into()?;
        Ok(match self { Self::Wii46 => u32::from_be_bytes(raw), Self::Pc72 => u32::from_le_bytes(raw) })
    }
}
pub struct Object<'a> { pub id: u32, pub kind: u32, pub payload: &'a [u8] }
pub struct Media<'a> { pub id: u32, pub bytes: &'a [u8] }
pub struct Bank<'a> {
    pub format: Format,
    pub header: &'a [u8],
    // Preserve original order and type-specific identities. Init.bnk uses
    // the same numeric ID for different HIRC types; IDs are not globally unique.
    pub objects: Vec<Object<'a>>,
    // Preserve even repeated zero-length records rather than silently dropping
    // padding/placeholder entries found in original PC banks.
    pub media: Vec<Media<'a>>,
}
impl<'a> Bank<'a> {
    pub fn parse(bytes: &'a [u8], format: Format) -> Result<Self> {
        let (mut header, mut hierarchy, mut index, mut data) = (None, None, None, None);
        let mut at = 0usize;
        while at < bytes.len() {
            let prefix_end = at.checked_add(8).context("Bank chunk offset overflow")?;
            let prefix = bytes.get(at..prefix_end).context("Truncated bank chunk header")?;
            let size = format.word(prefix, 4)? as usize;
            let end = prefix_end.checked_add(size).context("Bank chunk size overflow")?;
            let body = bytes.get(prefix_end..end).context("Truncated bank chunk")?;
            let slot = match &prefix[..4] {
                b"BKHD" => Some(&mut header), b"HIRC" => Some(&mut hierarchy),
                b"DIDX" => Some(&mut index), b"DATA" => Some(&mut data), _ => None,
            };
            if let Some(slot) = slot { ensure!(slot.replace(body).is_none(), "Duplicate bank chunk {:?}", &prefix[..4]); }
            at = end;
        }
        let header = header.context("Missing BKHD")?;
        let expected = match format { Format::Wii46 => 46, Format::Pc72 => 72 };
        ensure!(format.word(header, 0)? == expected, "Unsupported bank version for {format:?}");
        ensure!(header.len() >= 16, "Truncated bank metadata");
        let body = hierarchy.unwrap_or(&[0; 4]);
        let count = format.word(body, 0)?;
        let kind_width = match format { Format::Wii46 => 4, Format::Pc72 => 1 };
        let mut at = 4usize;
        let mut objects = Vec::new();
        for _ in 0..count {
            let kind = match format {
                Format::Wii46 => format.word(body, at)?,
                Format::Pc72 => u32::from(*body.get(at).context("Truncated HIRC type")?),
            };
            let size = format.word(body, at + kind_width)? as usize;
            ensure!(size >= 4, "HIRC object omits identity");
            let start = at.checked_add(kind_width + 4).context("HIRC offset overflow")?;
            let end = start.checked_add(size).context("HIRC size overflow")?;
            let record = body.get(start..end).context("Truncated HIRC object")?;
            let id = format.word(record, 0)?;
            objects.push(Object {id, kind, payload: &record[4..]});
            at = end;
        }
        ensure!(at == body.len(), "HIRC count does not account for all bytes");
        let mut media = Vec::new();
        if let Some(index) = index {
            ensure!(index.len() % 12 == 0, "Incomplete DIDX record");
            let data = data.context("DIDX has no DATA chunk")?;
            for record in index.chunks_exact(12) {
                let id = format.word(record, 0)?;
                let offset = format.word(record, 4)? as usize;
                let end = offset.checked_add(format.word(record, 8)? as usize).context("Media range overflow")?;
                let bytes = data.get(offset..end).with_context(|| format!("Media {id:08x} exceeds DATA chunk"))?;
                media.push(Media {id, bytes});
            }
        }
        Ok(Self {format, header, objects, media})
    }
    /// Both verified formats store the event's action count as a word. The
    /// identity was consumed by HIRC framing and is excluded from payload.
    pub fn event_actions(&self, id: u32) -> Result<Vec<u32>> {
        let mut matches = self.objects.iter().filter(|o| o.id == id && o.kind == 4);
        let object = matches.next().context("Missing audio event")?;
        ensure!(matches.next().is_none(), "Ambiguous audio event identity");
        let count = self.format.word(object.payload, 0)? as usize;
        ensure!(count.checked_mul(4).and_then(|n| n.checked_add(4)) == Some(object.payload.len()), "Malformed event action list");
        (0..count).map(|i| self.format.word(object.payload, 4 + i * 4)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(format: Format) -> Vec<u8> {
        let word = |n: u32| match format { Format::Wii46 => n.to_be_bytes(), Format::Pc72 => n.to_le_bytes() };
        let mut bytes = Vec::new();
        let mut chunk = |name: &[u8], body: &[u8]| {bytes.extend(name);bytes.extend(word(body.len() as u32));bytes.extend(body);};
        let mut header = Vec::new();
        for n in [if format == Format::Wii46 {46} else {72}, 7, 0, 0] {header.extend(word(n));}
        chunk(b"BKHD", &header);
        let mut hirc = word(1).to_vec();
        if format == Format::Wii46 {hirc.extend(word(4));} else {hirc.push(4);}
        for n in [12, 9, 1, 10] {hirc.extend(word(n));}
        chunk(b"HIRC", &hirc);
        bytes
    }
    #[test]
    fn layouts_share_container_logic_without_reinterpreting_payloads() -> Result<()> {
        for format in [Format::Wii46, Format::Pc72] {
            let bytes = fixture(format);
            let bank = Bank::parse(&bytes, format)?;
            assert_eq!(bank.event_actions(9)?, [10]);
            assert!(Bank::parse(&bytes[..bytes.len()-1], format).is_err());
            assert!(Bank::parse(&bytes, if format == Format::Wii46 {Format::Pc72} else {Format::Wii46}).is_err());
            let mut duplicate = bytes.clone();duplicate.extend(&bytes[24..]);
            assert!(Bank::parse(&duplicate, format).is_err());
        }
        Ok(())
    }
    #[test]
    fn pc_container_preserves_type_specific_ids_and_repeated_empty_media() -> Result<()> {
        let mut bytes=fixture(Format::Pc72);
        let mut second=bytes[36..].to_vec();
        bytes[36]=20;
        second[0]=18;
        bytes.extend(second);
        bytes[28..32].copy_from_slice(&38u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend(b"DIDX");bytes.extend(24u32.to_le_bytes());bytes.extend([0;24]);
        bytes.extend(b"DATA");bytes.extend(0u32.to_le_bytes());
        let bank=Bank::parse(&bytes,Format::Pc72)?;
        assert_eq!(bank.objects.iter().map(|o|(o.kind,o.id)).collect::<Vec<_>>(),[(20,9),(18,9)]);
        assert_eq!(bank.media.len(),2);
        assert!(bank.media.iter().all(|m|m.id==0 && m.bytes.is_empty()));
        Ok(())
    }
}
