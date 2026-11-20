use super::super::android_pcm::Handle;
use super::{ensure, Result, FRAMES, QUEUED_BLOCKS, RATE};
use std::{
    rc::Rc,
    time::{Duration, Instant},
};

pub struct Endpoint(Rc<Handle>);
impl Endpoint {
    pub fn open() -> Result<Self> {
        let stream = Handle::open(RATE, 2, (FRAMES * (QUEUED_BLOCKS + 1)) as u32)?;
        stream.start()?;
        Ok(Self(Rc::new(stream)))
    }
    pub fn submit(&self, samples: Vec<i16>) -> Result<Buffer> {
        let mut offset = 0;
        let deadline = Instant::now() + Duration::from_millis(100);
        while offset < samples.len() {
            let timeout = deadline.saturating_duration_since(Instant::now());
            ensure!(!timeout.is_zero(), "Android audio write timed out");
            offset += self.0.write(&samples[offset..], 2, timeout)?;
        }
        Ok(Buffer {
            stream: self.0.clone(),
            end_frame: self.0.written(),
        })
    }
}

pub struct Buffer {
    stream: Rc<Handle>,
    end_frame: u64,
}
impl Buffer {
    pub fn done(&self) -> bool {
        self.stream.consumed() >= self.end_frame
    }
}
