use super::super::android_pcm::Handle;
use anyhow::{ensure, Result};
use std::{collections::VecDeque, time::Duration};

pub struct Stream {
    pub rate: u32,
    pub channels: u16,
    pub submitted: u64,
    pub started: bool,
    endpoint: Handle,
    pending: VecDeque<Vec<i16>>,
    offset: usize,
}
impl Stream {
    pub fn open(rate: u32, channels: u16) -> Result<Self> {
        Ok(Self {
            rate,
            channels,
            submitted: 0,
            started: false,
            endpoint: Handle::open(rate, channels, rate)?,
            pending: VecDeque::new(),
            offset: 0,
        })
    }
    pub fn queue(&mut self, samples: Vec<i16>) -> Result<()> {
        let channels = usize::from(self.channels);
        ensure!(
            channels != 0 && samples.len() % channels == 0,
            "Invalid movie PCM block"
        );
        self.submitted += (samples.len() / channels) as u64;
        self.pending.push_back(samples);
        self.poll()
    }
    pub fn play(&mut self) -> Result<()> {
        if !self.started {
            self.endpoint.start()?;
            self.started = true;
        }
        Ok(())
    }
    pub fn poll(&mut self) -> Result<()> {
        while let Some(block) = self.pending.front() {
            self.offset += self.endpoint.write(
                &block[self.offset..],
                usize::from(self.channels),
                Duration::ZERO,
            )?;
            if self.offset < block.len() {
                break;
            }
            self.pending.pop_front();
            self.offset = 0;
        }
        Ok(())
    }
    pub fn position(&self) -> Result<u64> {
        Ok(self.endpoint.consumed().min(self.endpoint.written()))
    }
    pub fn empty(&self) -> bool {
        self.pending.is_empty() && self.endpoint.consumed() >= self.submitted
    }
    pub fn queued_frames(&self) -> Result<u64> {
        Ok(self.submitted.saturating_sub(self.position()?))
    }
}
