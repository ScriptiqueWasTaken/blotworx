//! Bounded external mono DSP reader. Keeps metadata, one encoded frame,
//! two decoded chunks and an8KiB IO buffer, never a whole recording.
use super::{audio_dsp::{Definition,Decoder},audio_loop::Layout};
use anyhow::{ensure,Context,Result};
use std::{fs::File,io::{Read,Seek,SeekFrom,BufReader},path::{Path,PathBuf},collections::{BTreeSet,VecDeque}};
#[derive(Clone,Debug)]
pub struct Spec {pub path:PathBuf,pub definition:Definition,data:u64,bytes:u64,file_bytes:u64}
impl Spec {
    pub fn read(path:&Path)->Result<Self> {
        let mut f=File::open(path).with_context(||format!("Open external audio {}",path.display()))?;let file_bytes=f.metadata()?.len();
        let mut header=[0;12];f.read_exact(&mut header)?;
        ensure!(&header[..4]==b"RIFX" && &header[8..]==b"WAVE","Expected external RIFX/WAVE audio");
        ensure!(u32::from_be_bytes(header[4..8].try_into()?) as u64+8<=file_bytes,"Truncated external audio");
        let mut at=12u64;let mut seen=BTreeSet::new();let mut fmt=None;let mut wii=None;let mut points=None;let mut data=None;
        while at<file_bytes {
            ensure!(file_bytes-at>=8,"Truncated external chunk header");f.seek(SeekFrom::Start(at))?;let mut h=[0;8];f.read_exact(&mut h)?;
            let tag:[u8;4]=h[..4].try_into()?;ensure!(seen.insert(tag),"Duplicate external audio chunk");
            let size=u32::from_be_bytes(h[4..].try_into()?) as u64;let body=at+8;let end=body.checked_add(size).context("External chunk overflow")?;ensure!(end<=file_bytes,"Truncated external audio chunk");
            match &tag {
                b"fmt "=>{ensure!(size==40,"Unsupported external DSP fmt layout");let mut v=[0;40];f.read_exact(&mut v)?;fmt=Some(v);},
                b"WiiH"=>{ensure!(size==46,"Unsupported external WiiH layout");let mut v=[0;46];f.read_exact(&mut v)?;wii=Some(v);},
                b"smpl"=>{
                    ensure!(size>=36,"Truncated external sampler");let mut v=[0;36];f.read_exact(&mut v)?;
                    if u32::from_be_bytes(v[28..32].try_into()?)!=0 {let offset=u32::from_be_bytes(v[32..].try_into()?) as u64;ensure!(offset+52<=size,"External loop record exceeds sampler");f.seek(SeekFrom::Start(body+offset+44))?;let mut p=[0;8];f.read_exact(&mut p)?;points=Some([u32::from_be_bytes(p[..4].try_into()?),u32::from_be_bytes(p[4..].try_into()?)]);}
                },
                b"data"=>{ensure!(size%8==0,"Partial external DSP frame");data=Some((body,size));},
                _=>{},
            }
            at=if end==file_bytes {end}else {end+(size&1)};ensure!(at<=file_bytes,"Missing external RIFF padding");
        }
        let mut definition=Definition::parse(&fmt.context("External audio fmt missing")?,&wii.context("External DSP coefficients missing")?,None)?;definition.loop_points=points;
        let (data,bytes)=data.context("External DSP data missing")?;
        ensure!(definition.sample_count>0 && definition.sample_count as u64<=bytes/8*14,"External sample count exceeds DSP data");
        Ok(Self{path:path.to_owned(),definition,data,bytes,file_bytes})
    }
    pub fn open(&self,layout:Layout)->Result<Reader> {
        let f=File::open(&self.path).with_context(||format!("Open streaming voice {}",self.path.display()))?;
        ensure!(f.metadata()?.len()==self.file_bytes,"External audio changed after preparation");
        ensure!(layout.samples==self.definition.sample_count as usize,"External voice layout disagrees with media");
        Ok(Reader{file:BufReader::with_capacity(8192,f),spec:self.clone(),layout,decoder:self.definition.decoder.clone(),time:0,next_raw:0,next_frame:None,encoded:None,cache:VecDeque::with_capacity(2),read_bytes:0})
    }
}
struct Chunk {time:u64,len:usize,samples:[i16;14]}
pub struct Reader {
    file:BufReader<File>,spec:Spec,layout:Layout,decoder:Decoder,time:u64,next_raw:usize,
    next_frame:Option<u64>,encoded:Option<(u64,[u8;8])>,cache:VecDeque<Chunk>,pub read_bytes:u64,
}
impl Reader {
    pub fn sample(&mut self,time:u64)->Result<Option<i16>> {
        if self.layout.index(time).is_none() {return Ok(None);}
        if self.cache.front().is_some_and(|c|time<c.time) {self.time=0;self.next_raw=0;self.decoder=self.spec.definition.decoder.clone();self.cache.clear();}
        while self.time<=time {
            let raw=self.layout.index(self.time).context("Stream ended before requested sample")?;let mut context=None;
            if raw!=self.next_raw {
                let [ps,a,b]=self.spec.definition.loop_context;ensure!(ps<=255,"Unsupported DSP loop predictor word");self.decoder.history=[a as i16,b as i16];context=Some(ps as u8);
            }
            let frame=raw as u64/14;let first=raw%14;let mut len=1;
            while first+len<14 && self.layout.index(self.time+len as u64)==Some(raw+len) {len+=1;}
            let encoded=if let Some((_,bytes))=self.encoded.filter(|(id,_)|*id==frame) {bytes}else {
                ensure!(frame*8+8<=self.spec.bytes,"External DSP address exceeds data");
                if self.next_frame!=Some(frame) {self.file.seek(SeekFrom::Start(self.spec.data+frame*8))?;}
                let mut bytes=[0;8];self.file.read_exact(&mut bytes).with_context(||format!("Read DSP frame {frame} in {}",self.spec.path.display()))?;self.read_bytes+=8;self.next_frame=Some(frame+1);self.encoded=Some((frame,bytes));bytes
            };
            let decoded=self.decoder.range(&encoded,first,first+len,context).with_context(||format!("Decode DSP frame {frame} in {}",self.spec.path.display()))?;
            let mut samples=[0;14];samples[..len].copy_from_slice(&decoded[first..first+len]);
            if self.cache.len()==2 {self.cache.pop_front();}self.cache.push_back(Chunk{time:self.time,len,samples});
            self.time+=len as u64;self.next_raw=raw+len;
        }
        self.cache.iter().find_map(|c|time.checked_sub(c.time).filter(|&i|i<(c.len as u64)).map(|i|c.samples[i as usize])).map(Some).context("External stream cache lost requested sample")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore="requires owned external DSP media"]
    fn original_external_reader_matches_decoded_samples_and_keeps_bounded_cache()->Result<()> {
        let root=std::env::var_os("BLOTWORX_TEST_DATA").context("Set BLOTWORX_TEST_DATA")?;let path=Path::new(&root).join("Audio/Wii/28204869.wav");
        let raw=std::fs::read(&path)?;let media=super::super::audio_dsp::Media::parse(&raw)?;let expected=media.decode()?;let spec=Spec::read(&path)?;
        assert_eq!(spec.definition.sample_rate,28000);let mut reader=spec.open(Layout::new(media.sample_count as usize,media.loop_points,1)?)?;
        for i in 0..media.sample_count as usize {assert_eq!(reader.sample(i as u64)?,Some(expected[i]),"sample{i}");assert!(reader.cache.len()<=2);}
        assert_eq!(reader.sample(media.sample_count as u64)?,None);assert!(reader.read_bytes<=spec.bytes);
        for i in [0,15,1,1000,50] {assert_eq!(reader.sample(i)?,Some(expected[i as usize]));}Ok(())
    }
    #[test]
    fn partial_frame_loop_restores_authored_predictor_history_and_tail()->Result<()> {
        let path=std::env::temp_dir().join(format!("blotworx-dsp-loop-{}.wav",std::process::id()));
        let mut fmt=[0u8;40];fmt[..2].copy_from_slice(&0xfff0u16.to_be_bytes());fmt[2..4].copy_from_slice(&1u16.to_be_bytes());fmt[4..8].copy_from_slice(&32000u32.to_be_bytes());fmt[8..12].copy_from_slice(&14u32.to_be_bytes());fmt[12..14].copy_from_slice(&8u16.to_be_bytes());fmt[14..16].copy_from_slice(&4u16.to_be_bytes());
        let mut wii=[0u8;46];wii[..2].copy_from_slice(&1024i16.to_be_bytes());wii[42..44].copy_from_slice(&100i16.to_be_bytes());wii[44..].copy_from_slice(&(-50i16).to_be_bytes());
        let mut sampler=[0u8;60];sampler[28..32].copy_from_slice(&1u32.to_be_bytes());sampler[44..48].copy_from_slice(&2u32.to_be_bytes());sampler[48..52].copy_from_slice(&5u32.to_be_bytes());let frame=[0,0x12,0x34,0x56,0x78,0x9a,0xbc,0xde];
        let mut raw=b"RIFX\0\0\0\0WAVE".to_vec();for (tag,bytes) in [(b"fmt ",fmt.as_slice()),(b"WiiH",wii.as_slice()),(b"smpl",sampler.as_slice()),(b"data",frame.as_slice())] {raw.extend(tag);raw.extend((bytes.len() as u32).to_be_bytes());raw.extend(bytes);}let size=(raw.len()-8) as u32;raw[4..8].copy_from_slice(&size.to_be_bytes());std::fs::write(&path,raw)?;
        let result=(||->Result<()>{let spec=Spec::read(&path)?;let mut reader=spec.open(Layout::new(14,Some([2,5]),2)?)?;let mut initial=spec.definition.decoder.clone();let a=initial.frame(&frame)?;let mut loop_decoder=spec.definition.decoder.clone();loop_decoder.history=[100,-50];let b=loop_decoder.range(&frame,2,14,Some(0))?;let expected=a[..5].iter().chain(b[2..].iter()).copied().collect::<Vec<_>>();for (i,v) in expected.iter().enumerate(){assert_eq!(reader.sample(i as u64)?,Some(*v));}assert_eq!(reader.sample(17)?,None);Ok(())})();let _=std::fs::remove_file(path);result
    }
}
