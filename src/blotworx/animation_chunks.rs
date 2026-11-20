//! PC animations.chf:32byte header,12byte signed-key sorted index.
//! Keep only the index and read the requested payload range on demand.
use anyhow::{ensure,Context,Result};
use std::{fs::File,io::{Read,Seek,SeekFrom},path::Path,sync::Mutex};
struct Row {key:i32,offset:u64,length:usize}
pub(super) struct Archive {file:Mutex<File>,rows:Vec<Row>}
impl Archive {
 pub fn open(path:&Path)->Result<Self> {
  let mut file=File::open(path)?;let size=file.metadata()?.len();let mut header=[0;32];file.read_exact(&mut header)?;
  let word=|offset|u32::from_le_bytes(header[offset..offset+4].try_into().unwrap());
  ensure!(word(0)==3598344017&&word(8)==2&&word(12)==32,"Unsupported animation CHF header");
  let bytes=(word(4) as usize).checked_mul(12).context("CHF index size overflow")?;
  ensure!(32u64.checked_add(bytes as u64).is_some_and(|end|end<=size),"Truncated CHF index");
  let mut table=vec![0;bytes];file.read_exact(&mut table)?;let mut rows=Vec::with_capacity(bytes/12);
  for row in table.chunks_exact(12) {
   let key=i32::from_le_bytes(row[..4].try_into().unwrap());let offset=u32::from_le_bytes(row[4..8].try_into().unwrap()) as u64;let length=u32::from_le_bytes(row[8..12].try_into().unwrap()) as usize;
   ensure!(offset.checked_add(length as u64).is_some_and(|end|end<=size),"CHF payload exceeds source file");
   ensure!(rows.last().is_none_or(|previous:&Row|previous.key<key),"CHF keys are not strictly signed-sorted");
   rows.push(Row {key,offset,length});
  }
  Ok(Self {file:Mutex::new(file),rows})
 }
 pub fn read_id(&self,id:u32)->Result<Option<Vec<u8>>> {
  let Ok(index)=self.rows.binary_search_by_key(&(id as i32),|row|row.key)else{return Ok(None)};
  let row=&self.rows[index];let mut file=self.file.lock().map_err(|_|anyhow::anyhow!("CHF file lock poisoned"))?;
  file.seek(SeekFrom::Start(row.offset))?;let mut bytes=vec![0;row.length];file.read_exact(&mut bytes)?;Ok(Some(bytes))
 }
 pub fn read(&self,path:&str)->Result<Option<Vec<u8>>> {
  let name=path.rsplit(['/', '\\']).next().context("Missing CHF filename")?;
  ensure!(name.is_ascii(),"Non-ASCII CHF name requires native locale support");
  let mut crc=!0u32;for byte in name.bytes().map(|b|b.to_ascii_lowercase()){crc^=byte as u32;for _ in 0..8{crc=(crc>>1)^(0xedb88320u32.wrapping_mul(crc&1));}}
  self.read_id(!crc)
 }
}

/// All payloads resident variant of b6db40/b6d290/b6d3c0 sampling state.
/// Caller retains readiness/IO ownership;this does not simulate async queues.
#[derive(Default)]
pub(super) struct Cursor {pub index:usize,pub offset:f32}
impl Cursor {
 pub fn select(&mut self,time:f32,durations:&[f32])->Result<(usize,f32)> {
  ensure!(time.is_finite()&&!durations.is_empty()&&durations.iter().all(|d|d.is_finite()&&*d>0.),"Invalid split sample clock/durations");
  if self.index!=0&&(time==0.||time<self.offset){self.index=0;self.offset=0.;}
  let local=(time as f64-self.offset as f64)as f32;
  if durations.len()>1&&local>durations[self.index] {
   self.offset=(self.offset as f64+durations[self.index]as f64)as f32;
   self.index=(self.index+1)%durations.len();
  }
  Ok((self.index,(time as f64-self.offset as f64)as f32))
 }
}
