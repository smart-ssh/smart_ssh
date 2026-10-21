//! Audio capture behind a trait (Spec 0108, section 5). The recorder only
//! sees `AudioSource`; the cpal device lives at the edge. Audio stays in
//! memory and is never written to disk or logged.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, Sample, SampleFormat};

/// Whisper expects 16 kHz mono.
pub const TARGET_RATE: u32 = 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureError {
    NoDevice,
    PermissionDenied,
    Failed,
}

/// Opens the input device on `open` and nowhere else.
pub trait AudioSource: Send + Sync {
    /// `max_samples` is a bound in 16 kHz samples; the capture stops
    /// collecting once it is reached.
    fn open(&self, max_samples: usize) -> Result<Box<dyn AudioCapture>, CaptureError>;
}

/// A running capture. Dropping it discards the audio.
pub trait AudioCapture: Send {
    /// Stops the device and returns the audio as 16 kHz mono.
    fn finish(self: Box<Self>) -> Vec<f32>;

    /// The audio captured so far as 16 kHz mono, without stopping.
    fn snapshot(&self) -> Vec<f32>;
}

/// Mixes interleaved frames down to mono and resamples to 16 kHz.
pub fn to_mono_16k(interleaved: &[f32], channels: usize, rate: u32) -> Vec<f32> {
    let channels = channels.max(1);
    let mono: Vec<f32> = interleaved
        .chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect();
    resample_mono(&mono, rate)
}

/// Resamples mono audio to 16 kHz (box average when downsampling, linear
/// interpolation when upsampling).
pub fn resample_mono(mono: &[f32], rate: u32) -> Vec<f32> {
    if rate == 0 || mono.is_empty() {
        return Vec::new();
    }
    if rate == TARGET_RATE {
        return mono.to_vec();
    }
    let ratio = rate as f64 / TARGET_RATE as f64;
    let out_len = (mono.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let start = i as f64 * ratio;
        if ratio > 1.0 {
            let a = start.floor() as usize;
            let b = (((i + 1) as f64 * ratio).floor() as usize)
                .max(a + 1)
                .min(mono.len());
            let slice = &mono[a..b];
            out.push(slice.iter().sum::<f32>() / slice.len() as f32);
        } else {
            let a = start.floor() as usize;
            let frac = (start - a as f64) as f32;
            let x = mono[a];
            let y = mono.get(a + 1).copied().unwrap_or(x);
            out.push(x + (y - x) * frac);
        }
    }
    out
}

fn map_cpal(kind: ErrorKind) -> CaptureError {
    match kind {
        ErrorKind::PermissionDenied => CaptureError::PermissionDenied,
        ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable => CaptureError::NoDevice,
        _ => CaptureError::Failed,
    }
}

/// Default input device via cpal.
#[derive(Default)]
pub struct CpalSource;

struct CpalCapture {
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
    buf: Arc<Mutex<Vec<f32>>>,
    rate: u32,
}

impl AudioSource for CpalSource {
    fn open(&self, max_samples: usize) -> Result<Box<dyn AudioCapture>, CaptureError> {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, CaptureError>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let thread_buf = Arc::clone(&buf);
        // cpal streams are not `Send`: the stream lives on its own thread.
        let thread = std::thread::spawn(move || {
            match build_stream(max_samples, thread_buf) {
                Ok((stream, rate)) => {
                    let _ = ready_tx.send(Ok(rate));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            };
        });
        match ready_rx.recv() {
            Ok(Ok(rate)) => Ok(Box::new(CpalCapture {
                stop: stop_tx,
                thread: Some(thread),
                buf,
                rate,
            })),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(CaptureError::Failed),
        }
    }
}

fn push_frames<T: Sample>(data: &[T], channels: usize, cap: usize, buf: &Mutex<Vec<f32>>)
where
    f32: cpal::FromSample<T>,
{
    let Ok(mut guard) = buf.lock() else { return };
    for frame in data.chunks_exact(channels) {
        if guard.len() >= cap {
            return;
        }
        let sum: f32 = frame.iter().map(|s| f32::from_sample(*s)).sum();
        guard.push(sum / channels as f32);
    }
}

fn build_stream(
    max_samples: usize,
    buf: Arc<Mutex<Vec<f32>>>,
) -> Result<(cpal::Stream, u32), CaptureError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(CaptureError::NoDevice)?;
    let config = device
        .default_input_config()
        .map_err(|e| map_cpal(e.kind()))?;
    let rate: u32 = config.sample_rate() as u32;
    let channels = config.channels() as usize;
    let format = config.sample_format();
    // Bound in device-rate mono samples.
    let cap = (max_samples as u64 * rate as u64 / TARGET_RATE as u64) as usize;
    let err_fn = |_e: cpal::Error| tracing::warn!("audio stream error");
    let cfg: cpal::StreamConfig = config.into();
    let stream = match format {
        SampleFormat::I16 => device.build_input_stream(
            cfg,
            move |d: &[i16], _: &_| push_frames(d, channels, cap, &buf),
            err_fn,
            None,
        ),
        SampleFormat::I32 => device.build_input_stream(
            cfg,
            move |d: &[i32], _: &_| push_frames(d, channels, cap, &buf),
            err_fn,
            None,
        ),
        SampleFormat::F32 => device.build_input_stream(
            cfg,
            move |d: &[f32], _: &_| push_frames(d, channels, cap, &buf),
            err_fn,
            None,
        ),
        _ => return Err(CaptureError::Failed),
    }
    .map_err(|e| map_cpal(e.kind()))?;
    stream.play().map_err(|e| map_cpal(e.kind()))?;
    Ok((stream, rate))
}

impl AudioCapture for CpalCapture {
    fn snapshot(&self) -> Vec<f32> {
        let samples = self.buf.lock().map(|b| b.clone()).unwrap_or_default();
        resample_mono(&samples, self.rate)
    }

    fn finish(mut self: Box<Self>) -> Vec<f32> {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let samples = std::mem::take(&mut *self.buf.lock().expect("audio buffer lock"));
        resample_mono(&samples, self.rate)
    }
}

impl Drop for CpalCapture {
    fn drop(&mut self) {
        // Also reached on `finish`; harmless the second time.
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Ok(mut b) = self.buf.lock() {
            b.clear();
        }
    }
}
