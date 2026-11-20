//! Wwise version72 PC source/action records. Raw property bits and trailing
//! node parameters are retained; parsing is not action/node execution.
use anyhow::{ensure, Context, Result};
use super::audio_bank_framing::Format;
pub(super) struct Reader<'a> {pub bytes:&'a [u8],pub at:usize}
impl<'a> Reader<'a> {
    pub fn take(&mut self,size:usize)->Result<&'a [u8]> {
        let end=self.at.checked_add(size).context("Audio record size overflow")?;
        let bytes=self.bytes.get(self.at..end).context("Truncated modern audio record")?;self.at=end;Ok(bytes)
    }
    pub fn byte(&mut self)->Result<u8>{Ok(self.take(1)?[0])}
    pub fn word(&mut self)->Result<u32>{Format::Pc72.word(self.take(4)?,0)}
}
#[derive(Debug, serde::Serialize)]
pub struct Source<'a> {
    pub plugin:u32,pub stream_type:u32,pub media_id:u32,pub file_id:u32,
    pub memory:Option<[u32;2]>,pub flags:u8,
    #[serde(skip)] pub plugin_parameters:Option<&'a [u8]>,
    #[serde(skip)] pub node_parameters:&'a [u8],
}
impl<'a> Source<'a> {
    pub fn parse(bytes:&'a [u8])->Result<Self> {
        let mut r=Reader {bytes,at:0};
        let plugin=r.word()?;let stream_type=r.word()?;let media_id=r.word()?;let file_id=r.word()?;
        let memory=if stream_type==1 {None}else{Some([r.word()?,r.word()?])};
        let flags=r.byte()?;
        let kind=plugin&15;
        ensure!(matches!(kind,0|1|2|5),"Unsupported modern audio plugin kind {kind}");
        ensure!(kind!=1 || stream_type<=2,"Unsupported modern codec stream mode {stream_type}");
        let plugin_parameters=if matches!(kind,2|5) {let size=r.word()? as usize;Some(r.take(size)?)}else{None};
        Ok(Self {plugin,stream_type,media_id,file_id,memory,flags,plugin_parameters,node_parameters:&bytes[r.at..]})
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Property {pub id:u8,pub bits:u32}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Range {pub id:u8,pub bits:[u32;2]}
#[derive(Debug, serde::Serialize)]
pub struct Action<'a> {
    pub kind:u16,pub target:u32,pub target_flags:u8,pub properties:Vec<Property>,pub ranges:Vec<Range>,
    #[serde(skip)] pub specific:&'a [u8],
}
impl<'a> Action<'a> {
    pub fn parse(bytes:&'a [u8])->Result<Self> {
        let mut r=Reader {bytes,at:0};let kind=u16::from_le_bytes(r.take(2)?.try_into()?);
        let target=r.word()?;let target_flags=r.byte()?;
        let (properties,ranges)=bundles(&mut r)?;
        Ok(Self {kind,target,target_flags,properties,ranges,specific:&bytes[r.at..]})
    }
    pub fn play_bank(&self)->Result<u32> {
        ensure!(self.kind&0xff00==0x0400,"Not a modern play action");
        ensure!(self.specific.len()==5,"Unsupported modern play action suffix");
        Format::Pc72.word(self.specific,1)
    }
}
pub(super) fn bundles(r:&mut Reader<'_>)->Result<(Vec<Property>,Vec<Range>)> {
    let count=r.byte()? as usize;let ids=r.take(count)?;
    let mut properties=Vec::new();for &id in ids {properties.push(Property {id,bits:r.word()?});}
    let count=r.byte()? as usize;let ids=r.take(count)?;
    let mut ranges=Vec::new();for &id in ids {ranges.push(Range {id,bits:[r.word()?,r.word()?]});}
    Ok((properties,ranges))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn modern_records_keep_byte_flags_and_do_not_read_legacy_rate_fields()->Result<()> {
        let source=[1u32,0,123,456,96,20].into_iter().flat_map(u32::to_le_bytes).chain([3,0xaa,0xbb]).collect::<Vec<_>>();
        let parsed=Source::parse(&source)?;
        assert_eq!((parsed.media_id,parsed.file_id,parsed.memory,parsed.flags),(123,456,Some([96,20]),3));
        assert_eq!(parsed.node_parameters,[0xaa,0xbb]);
        let action=[3,4,0x7d,0x4a,0xfd,0x0c,0,0,0,4,0x53,0x84,0xce,0x2f];
        let action=Action::parse(&action)?;
        assert_eq!(action.target,0x0cfd4a7d);assert_eq!(action.play_bank()?,0x2fce8453);
        Ok(())
    }
    #[test]
    fn truncated_headers_and_property_bundles_fail_instead_of_defaulting() {
        for size in 0..25 {assert!(Source::parse(&vec![0;size]).is_err());}
        assert!(Action::parse(&[3,4,1,0,0,0,0,2,1]).is_err());
    }
}
