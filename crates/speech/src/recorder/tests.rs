use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::audio::{resample_mono, to_mono_16k};
use crate::transcribe::{parse_language, InvalidLanguage, TranscribeError, Transcriber};

const WAV: &[u8] = include_bytes!("../testdata/short.wav");

/// Minimal decoder for the bundled 16 kHz mono PCM16 test file.
fn decode_wav(bytes: &[u8]) -> Vec<f32> {
    assert_eq!(&bytes[0..4], b"RIFF");
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
    assert_eq!(
        u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
        16_000
    );
    bytes[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
        .collect()
}

struct FakeCapture(Vec<f32>, Arc<AtomicUsize>);
impl AudioCapture for FakeCapture {
    fn finish(self: Box<Self>) -> Vec<f32> {
        self.0.clone()
    }
}
impl Drop for FakeCapture {
    fn drop(&mut self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

struct FakeSource {
    samples: Vec<f32>,
    err: Option<CaptureError>,
    opens: AtomicUsize,
    drops: Arc<AtomicUsize>,
    max_seen: Mutex<usize>,
}
impl FakeSource {
    fn build(samples: Vec<f32>, err: Option<CaptureError>) -> Arc<Self> {
        Arc::new(Self {
            samples,
            err,
            opens: AtomicUsize::new(0),
            drops: Arc::new(AtomicUsize::new(0)),
            max_seen: Mutex::new(0),
        })
    }
    fn new(samples: Vec<f32>) -> Arc<Self> {
        Self::build(samples, None)
    }
    fn failing(err: CaptureError) -> Arc<Self> {
        Self::build(vec![], Some(err))
    }
}
impl AudioSource for FakeSource {
    fn open(&self, max_samples: usize) -> Result<Box<dyn AudioCapture>, CaptureError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        *self.max_seen.lock().unwrap() = max_samples;
        if let Some(e) = self.err {
            return Err(e);
        }
        Ok(Box::new(FakeCapture(
            self.samples.clone(),
            Arc::clone(&self.drops),
        )))
    }
}

#[derive(Default)]
struct FakeTranscriber {
    calls: Mutex<Vec<(PathBuf, usize, Option<String>)>>,
    fail: bool,
}
impl Transcriber for FakeTranscriber {
    fn transcribe(
        &self,
        model: &Path,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, TranscribeError> {
        self.calls.lock().unwrap().push((
            model.to_owned(),
            samples.len(),
            language.map(str::to_owned),
        ));
        if self.fail {
            Err(TranscribeError)
        } else {
            Ok(format!("heard {} samples", samples.len()))
        }
    }
}

type Events = Arc<Mutex<Vec<RecorderState>>>;

fn recorder(
    source: Arc<FakeSource>,
    tr: Arc<FakeTranscriber>,
    max: Duration,
) -> (Recorder, Events) {
    let events: Events = Arc::default();
    let ev = Arc::clone(&events);
    let rec = Recorder::new(
        source,
        tr,
        max,
        Arc::new(move |s| ev.lock().unwrap().push(s)),
        tokio::runtime::Handle::current(),
    );
    (rec, events)
}

fn model() -> Option<PathBuf> {
    Some(PathBuf::from("/models/ggml-base.bin"))
}

#[tokio::test]
async fn start_stop_returns_transcribed_text_from_wav() {
    let samples = decode_wav(WAV);
    let n = samples.len();
    let tr = Arc::new(FakeTranscriber::default());
    let (rec, events) = recorder(FakeSource::new(samples), Arc::clone(&tr), MAX_RECORDING);
    rec.start(model(), None).unwrap();
    assert_eq!(rec.stop().await.unwrap(), format!("heard {n} samples"));
    assert_eq!(
        tr.calls.lock().unwrap()[0].0,
        PathBuf::from("/models/ggml-base.bin")
    );
    assert_eq!(
        *events.lock().unwrap(),
        [
            RecorderState::Recording,
            RecorderState::Transcribing,
            RecorderState::Idle
        ]
    );
}

#[tokio::test]
async fn start_without_model_errors_and_does_not_open_device() {
    let src = FakeSource::new(vec![0.0; 100]);
    let (rec, _) = recorder(Arc::clone(&src), Arc::default(), MAX_RECORDING);
    assert_eq!(rec.start(None, None), Err(RecordError::NoModel));
    assert_eq!(src.opens.load(Ordering::SeqCst), 0);
    assert_eq!(rec.stop().await, Err(RecordError::NotRecording));
}

#[tokio::test]
async fn cancel_discards_audio_and_transcribes_nothing() {
    let src = FakeSource::new(decode_wav(WAV));
    let tr = Arc::new(FakeTranscriber::default());
    let (rec, events) = recorder(Arc::clone(&src), Arc::clone(&tr), MAX_RECORDING);
    rec.start(model(), None).unwrap();
    rec.cancel();
    assert_eq!(src.drops.load(Ordering::SeqCst), 1, "capture dropped");
    assert!(tr.calls.lock().unwrap().is_empty());
    assert_eq!(rec.stop().await, Err(RecordError::NotRecording));
    assert_eq!(events.lock().unwrap().last(), Some(&RecorderState::Idle));
}

#[tokio::test]
async fn second_start_errors_and_first_recording_continues() {
    let src = FakeSource::new(decode_wav(WAV));
    let (rec, _) = recorder(Arc::clone(&src), Arc::default(), MAX_RECORDING);
    rec.start(model(), None).unwrap();
    assert_eq!(rec.start(model(), None), Err(RecordError::AlreadyRecording));
    assert_eq!(src.opens.load(Ordering::SeqCst), 1);
    assert!(rec.stop().await.unwrap().starts_with("heard"));
}

#[tokio::test]
async fn auto_stop_at_max_length_transcribes_and_stop_collects() {
    let src = FakeSource::new(decode_wav(WAV));
    let tr = Arc::new(FakeTranscriber::default());
    let (rec, events) = recorder(Arc::clone(&src), Arc::clone(&tr), Duration::from_millis(50));
    rec.start(model(), None).unwrap();
    assert_eq!(
        *src.max_seen.lock().unwrap(),
        800,
        "bound in 16 kHz samples"
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        events.lock().unwrap().last(),
        Some(&RecorderState::Finished)
    );
    assert_eq!(tr.calls.lock().unwrap().len(), 1);
    assert!(rec.stop().await.unwrap().starts_with("heard"));
    assert_eq!(rec.stop().await, Err(RecordError::NotRecording));
    // a new recording is possible afterwards
    rec.start(model(), None).unwrap();
    rec.cancel();
}

#[tokio::test]
async fn language_is_passed_through() {
    for lang in [None, Some("de".to_owned())] {
        let tr = Arc::new(FakeTranscriber::default());
        let (rec, _) = recorder(
            FakeSource::new(decode_wav(WAV)),
            Arc::clone(&tr),
            MAX_RECORDING,
        );
        rec.start(model(), lang.clone()).unwrap();
        rec.stop().await.unwrap();
        assert_eq!(tr.calls.lock().unwrap()[0].2, lang);
    }
}

#[tokio::test]
async fn device_errors_map_to_keys() {
    for (cap, key) in [
        (CaptureError::NoDevice, "speech_no_device"),
        (CaptureError::PermissionDenied, "speech_permission_denied"),
    ] {
        let (rec, _) = recorder(FakeSource::failing(cap), Arc::default(), MAX_RECORDING);
        assert_eq!(rec.start(model(), None).unwrap_err().key(), key);
        // a failed start leaves the recorder usable
        assert_eq!(rec.stop().await, Err(RecordError::NotRecording));
    }
}

#[tokio::test]
async fn transcription_failure_maps_to_key_and_frees_recorder() {
    let tr = Arc::new(FakeTranscriber {
        fail: true,
        ..Default::default()
    });
    let (rec, _) = recorder(FakeSource::new(decode_wav(WAV)), tr, MAX_RECORDING);
    rec.start(model(), None).unwrap();
    let err = rec.stop().await.unwrap_err();
    assert_eq!(err.key(), "speech_transcription_failed");
    rec.start(model(), None).unwrap();
    rec.cancel();
}

#[test]
fn resampling_halves_and_downmixes() {
    // 32 kHz stereo -> 16 kHz mono: 4 frames -> 2 samples
    let out = to_mono_16k(&[1.0, 0.0, 1.0, 0.0, 0.5, 0.5, 0.5, 0.5], 2, 32_000);
    assert_eq!(out, vec![0.5, 0.5]);
    assert_eq!(resample_mono(&[0.1, 0.2], 16_000), vec![0.1, 0.2]);
    assert_eq!(resample_mono(&[0.0, 1.0], 8_000).len(), 4);
    assert!(resample_mono(&[], 48_000).is_empty());
}

#[test]
fn language_setting_parses() {
    assert_eq!(parse_language("auto"), Ok(None));
    assert_eq!(parse_language(""), Ok(None));
    assert_eq!(parse_language("DE"), Ok(Some("de".to_owned())));
    assert_eq!(parse_language("xx-not"), Err(InvalidLanguage));
}

/// Real Whisper inference; needs a downloaded model:
/// `SMART_SSH_WHISPER_MODEL=/path/ggml-base.bin cargo test -p speech -- --ignored`
#[test]
#[ignore]
fn whisper_engine_runs_on_the_bundled_wav() {
    let Ok(path) = std::env::var("SMART_SSH_WHISPER_MODEL") else {
        return;
    };
    let t = crate::WhisperTranscriber;
    t.transcribe(Path::new(&path), &decode_wav(WAV), None)
        .unwrap();
}

/// Regression: `start` is called from a plain (non-async) Tauri command,
/// i.e. on a thread without a Tokio runtime context. The timer must be
/// spawned on the injected handle instead of panicking.
#[test]
fn start_works_outside_a_runtime_context() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let events: Events = Arc::default();
    let ev = Arc::clone(&events);
    let rec = Recorder::new(
        FakeSource::new(vec![0.1; 16_000]),
        Arc::new(FakeTranscriber::default()),
        Duration::from_secs(60),
        Arc::new(move |s| ev.lock().unwrap().push(s)),
        rt.handle().clone(),
    );
    // No `#[tokio::test]`: this thread has no runtime entered.
    rec.start(model(), None)
        .expect("start without runtime context");
    rec.cancel();
}
