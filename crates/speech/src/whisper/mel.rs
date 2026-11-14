//! Whisper's log-mel front end: 16 kHz mono in, `[n_mels, 3000]` features for
//! a 30 s window out. Same numbers as OpenAI's reference implementation
//! (Hann window 400, hop 160, reflect padding, Slaney mel filters).

use std::f32::consts::PI;

pub const N_FFT: usize = 400;
pub const HOP: usize = 160;
pub const WINDOW_SAMPLES: usize = 30 * 16_000;
pub const N_FRAMES: usize = WINDOW_SAMPLES / HOP;
const N_BINS: usize = N_FFT / 2 + 1;

pub struct MelFrontEnd {
    n_mels: usize,
    window: Vec<f32>,
    cos: Vec<f32>,
    sin: Vec<f32>,
    /// `[n_mels][N_BINS]`
    filters: Vec<f32>,
}

const F_SP: f64 = 200.0 / 3.0;
const MIN_LOG_HZ: f64 = 1000.0;

fn hz_to_mel(hz: f64) -> f64 {
    let min_log_mel = MIN_LOG_HZ / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if hz >= MIN_LOG_HZ {
        min_log_mel + (hz / MIN_LOG_HZ).ln() / logstep
    } else {
        hz / F_SP
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    let min_log_mel = MIN_LOG_HZ / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if mel >= min_log_mel {
        MIN_LOG_HZ * (logstep * (mel - min_log_mel)).exp()
    } else {
        mel * F_SP
    }
}

impl MelFrontEnd {
    pub fn new(n_mels: usize) -> Self {
        let window = (0..N_FFT)
            .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f32 / N_FFT as f32).cos())
            .collect();
        let mut cos = Vec::with_capacity(N_BINS * N_FFT);
        let mut sin = Vec::with_capacity(N_BINS * N_FFT);
        for k in 0..N_BINS {
            for n in 0..N_FFT {
                let a = 2.0 * std::f64::consts::PI * ((k * n) % N_FFT) as f64 / N_FFT as f64;
                cos.push(a.cos() as f32);
                sin.push(a.sin() as f32);
            }
        }
        // Slaney-scale triangular filters, area-normalised.
        let (lo, hi) = (hz_to_mel(0.0), hz_to_mel(8000.0));
        let mel_pts: Vec<f64> = (0..n_mels + 2)
            .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (n_mels + 1) as f64))
            .collect();
        let mut filters = vec![0.0f32; n_mels * N_BINS];
        for m in 0..n_mels {
            let (l, c, r) = (mel_pts[m], mel_pts[m + 1], mel_pts[m + 2]);
            let enorm = 2.0 / (r - l);
            for k in 0..N_BINS {
                let f = k as f64 * 16_000.0 / N_FFT as f64;
                let lower = (f - l) / (c - l);
                let upper = (r - f) / (r - c);
                filters[m * N_BINS + k] = (lower.min(upper).max(0.0) * enorm) as f32;
            }
        }
        Self {
            n_mels,
            window,
            cos,
            sin,
            filters,
        }
    }

    /// Features for at most 30 s of audio (longer input is cut), zero-padded to
    /// 30 s; row-major `[n_mels, N_FRAMES]`.
    pub fn features(&self, samples: &[f32]) -> Vec<f32> {
        let audio = &samples[..samples.len().min(WINDOW_SAMPLES)];
        let n = audio.len();
        // Sample `i` of the reflect-padded, zero-padded signal.
        let at = |i: isize| -> f32 {
            let j = i.unsigned_abs();
            if j < n {
                audio[j]
            } else {
                0.0
            }
        };
        // Frames whose whole span lies behind the audio are silent.
        let live_frames = (n + N_FFT / 2).div_ceil(HOP).min(N_FRAMES);
        let mut log_mel = vec![-10.0f32; self.n_mels * N_FRAMES];
        let mut frame = vec![0.0f32; N_FFT];
        let mut power = vec![0.0f32; N_BINS];
        for t in 0..live_frames {
            let start = (t * HOP) as isize - (N_FFT / 2) as isize;
            for (i, f) in frame.iter_mut().enumerate() {
                *f = at(start + i as isize) * self.window[i];
            }
            for (k, p) in power.iter_mut().enumerate() {
                let c = &self.cos[k * N_FFT..(k + 1) * N_FFT];
                let s = &self.sin[k * N_FFT..(k + 1) * N_FFT];
                let re: f32 = frame.iter().zip(c).map(|(a, b)| a * b).sum();
                let im: f32 = frame.iter().zip(s).map(|(a, b)| a * b).sum();
                *p = re * re + im * im;
            }
            for m in 0..self.n_mels {
                let row = &self.filters[m * N_BINS..(m + 1) * N_BINS];
                let e: f32 = row.iter().zip(&power).map(|(a, b)| a * b).sum();
                log_mel[m * N_FRAMES + t] = e.max(1e-10).log10();
            }
        }
        let max = log_mel.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        for v in &mut log_mel {
            *v = (v.max(max - 8.0) + 4.0) / 4.0;
        }
        log_mel
    }
}
