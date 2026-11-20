//! Original PC SplitHavokAnimHeader::Read b6e410 and Serializer b6e740..b6eace.
//! Network-endian scalars and each short occupy16 bytes; padding is opaque.
use anyhow::{ensure,Context,Result};
#[derive(Debug)]
pub(super) struct Header<'a> {
    pub version:u32,pub name:&'a str,pub duration:f32,
    pub num_chunks:u32,pub num_frames:u32,pub field_14:u32,
    pub transform_tracks:u32,pub float_tracks:u32,
    pub track_to_bone:Vec<i16>,pub float_to_slot:Vec<i16>,
    pub payload:&'a [u8],pub serialized_size:usize,
}
struct Reader<'a> {bytes:&'a [u8],cursor:usize}
impl<'a> Reader<'a> {
    fn span(&mut self,length:usize)->Result<&'a [u8]> {
        let padded=length.checked_add(15).context("Streamed animation span overflow")?&!15;
        let end=self.cursor.checked_add(padded).context("Streamed animation cursor overflow")?;
        ensure!(end<=self.bytes.len(),"Truncated streamed animation field at {}",self.cursor);
        let value=&self.bytes[self.cursor..self.cursor+length];self.cursor=end;Ok(value)
    }
    fn word(&mut self)->Result<u32> {Ok(u32::from_be_bytes(self.span(16)?[..4].try_into()?))}
    fn count(&mut self)->Result<usize> {let n=self.word()?;ensure!(n<=i32::MAX as u32,"Negative streamed animation count");Ok(n as usize)}
    fn blob(&mut self)->Result<&'a [u8]> {let n=self.count()?;self.span(n)}
    fn shorts(&mut self)->Result<Vec<i16>> {
        let n=self.count()?;ensure!(n<=(self.bytes.len()-self.cursor)/16,"Truncated streamed animation short array");
        (0..n).map(|_|Ok(i16::from_be_bytes(self.span(16)?[..2].try_into()?))).collect()
    }
}
impl<'a> Header<'a> {
    pub fn parse(bytes:&'a [u8])->Result<Self> {
        let mut r=Reader {bytes,cursor:0};let version=r.word()?;
        ensure!(version==7,"Unsupported split animation version {version}");
        let name=std::str::from_utf8(r.blob()?).context("Unsupported streamed animation name encoding")?;
        let duration=f32::from_bits(r.word()?);ensure!(duration.is_finite()&&duration>0.,"Invalid streamed animation duration");
        let num_chunks=r.word()?;let num_frames=r.word()?;let field_14=r.word()?;
        let transform_tracks=r.word()?;let float_tracks=r.word()?;
        let track_to_bone=r.shorts()?;let float_to_slot=r.shorts()?;let payload=r.blob()?;
        Ok(Self {version,name,duration,num_chunks,num_frames,field_14,transform_tracks,float_tracks,
            track_to_bone,float_to_slot,payload,serialized_size:r.cursor})
    }
}
