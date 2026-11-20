use anyhow::{ensure, Context, Result};
use std::{
    ffi::c_void,
    ptr::{null_mut, NonNull},
};

#[repr(C)]
struct StreamBuilder {
    _private: [u8; 0],
}
#[repr(C)]
struct Stream {
    _private: [u8; 0],
}

#[link(name = "aaudio")]
extern "C" {
    fn AAudio_createStreamBuilder(builder: *mut *mut StreamBuilder) -> i32;
    fn AAudioStreamBuilder_delete(builder: *mut StreamBuilder) -> i32;
    fn AAudioStreamBuilder_setSampleRate(builder: *mut StreamBuilder, rate: i32);
    fn AAudioStreamBuilder_setChannelCount(builder: *mut StreamBuilder, channels: i32);
    fn AAudioStreamBuilder_setFormat(builder: *mut StreamBuilder, format: i32);
    fn AAudioStreamBuilder_setBufferCapacityInFrames(builder: *mut StreamBuilder, frames: i32);
    fn AAudioStreamBuilder_openStream(builder: *mut StreamBuilder, stream: *mut *mut Stream)
        -> i32;
    fn AAudioStream_requestStart(stream: *mut Stream) -> i32;
    fn AAudioStream_close(stream: *mut Stream) -> i32;
    fn AAudioStream_write(
        stream: *mut Stream,
        data: *const c_void,
        frames: i32,
        timeout_ns: i64,
    ) -> i32;
    fn AAudioStream_getFramesWritten(stream: *mut Stream) -> i64;
    fn AAudioStream_getFramesRead(stream: *mut Stream) -> i64;
}

struct Builder(NonNull<StreamBuilder>);
impl Drop for Builder {
    fn drop(&mut self) {
        unsafe {
            AAudioStreamBuilder_delete(self.0.as_ptr());
        }
    }
}

pub(super) struct Handle(NonNull<Stream>);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            AAudioStream_close(self.0.as_ptr());
        }
    }
}

impl Handle {
    pub(super) fn open(rate: u32, channels: u16, capacity: u32) -> Result<Self> {
        const PCM_I16: i32 = 1;
        let mut builder = null_mut();
        let status = unsafe { AAudio_createStreamBuilder(&mut builder) };
        ensure!(
            status == 0,
            "Create Android audio stream builder ({status})"
        );
        let builder = Builder(
            NonNull::new(builder).ok_or_else(|| anyhow::anyhow!("No Android audio builder"))?,
        );
        let raw_builder = builder.0.as_ptr();
        let mut stream = null_mut();
        let status = unsafe {
            AAudioStreamBuilder_setSampleRate(raw_builder, i32::try_from(rate)?);
            AAudioStreamBuilder_setChannelCount(raw_builder, i32::from(channels));
            AAudioStreamBuilder_setFormat(raw_builder, PCM_I16);
            AAudioStreamBuilder_setBufferCapacityInFrames(raw_builder, i32::try_from(capacity)?);
            AAudioStreamBuilder_openStream(raw_builder, &mut stream)
        };
        ensure!(status == 0, "Open Android audio stream ({status})");
        let stream =
            Handle(NonNull::new(stream).ok_or_else(|| anyhow::anyhow!("No Android audio device"))?);
        Ok(stream)
    }
    pub(super) fn start(&self) -> Result<()> {
        let status = unsafe { AAudioStream_requestStart(self.0.as_ptr()) };
        ensure!(status == 0, "Start Android audio stream ({status})");
        Ok(())
    }
    pub(super) fn write(
        &self,
        samples: &[i16],
        channels: usize,
        timeout: std::time::Duration,
    ) -> Result<usize> {
        ensure!(
            channels > 0 && samples.len() % channels == 0,
            "Invalid PCM block length"
        );
        let frames = i32::try_from(samples.len() / channels)?;
        let timeout = i64::try_from(timeout.as_nanos()).context("Invalid audio timeout")?;
        let written = unsafe {
            AAudioStream_write(self.0.as_ptr(), samples.as_ptr().cast(), frames, timeout)
        };
        ensure!(
            (0..=frames).contains(&written),
            "Write Android audio stream ({written})"
        );
        Ok(written as usize * channels)
    }
    pub(super) fn written(&self) -> u64 {
        unsafe { AAudioStream_getFramesWritten(self.0.as_ptr()) }.max(0) as u64
    }
    pub(super) fn consumed(&self) -> u64 {
        unsafe { AAudioStream_getFramesRead(self.0.as_ptr()) }.max(0) as u64
    }
}
