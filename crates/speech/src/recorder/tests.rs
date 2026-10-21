use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;
use crate::audio::{resample_mono, to_mono_16k};
use crate::transcribe::{
    parse_language, InvalidLanguage, TranscribeError, TranscribeSession, Transcriber,
};

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
    fn snapshot(&self) -> Vec<f32> {
        self.0.clone()
    }
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

/// Blocks inside `transcribe` until released, to observe the busy state.
#[derive(Default)]
struct GatedTranscriber {
    entered: std::sync::atomic::AtomicBool,
    release: std::sync::atomic::AtomicBool,
}
impl Transcriber for GatedTranscriber {
    fn transcribe(&self, _: &Path, _: &[f32], _: Option<&str>) -> Result<String, TranscribeError> {
        self.entered.store(true, Ordering::SeqCst);
        while !self.release.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok("late text".to_owned())
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

async fn manual_stop_in_transcription() -> (
    Recorder,
    Arc<FakeSource>,
    Arc<GatedTranscriber>,
    Events,
    tokio::task::JoinHandle<Result<String, RecordError>>,
) {
    let src = FakeSource::new(decode_wav(WAV));
    let tr = Arc::new(GatedTranscriber::default());
    let events: Events = Arc::default();
    let ev = Arc::clone(&events);
    let rec = Recorder::new(
        src.clone(),
        tr.clone(),
        MAX_RECORDING,
        Arc::new(move |s| ev.lock().unwrap().push(s)),
        tokio::runtime::Handle::current(),
    );
    rec.start(model(), None).unwrap();
    let r2 = rec.clone();
    let stop = tokio::spawn(async move { r2.stop().await });
    while !tr.entered.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    (rec, src, tr, events, stop)
}

#[tokio::test]
async fn start_during_manual_transcription_is_refused() {
    let (rec, src, tr, _, stop) = manual_stop_in_transcription().await;
    assert_eq!(rec.start(model(), None), Err(RecordError::AlreadyRecording));
    assert_eq!(
        src.opens.load(Ordering::SeqCst),
        1,
        "microphone stays closed"
    );
    tr.release.store(true, Ordering::SeqCst);
    assert_eq!(stop.await.unwrap().unwrap(), "late text");
    rec.start(model(), None).unwrap(); // idle again
    rec.cancel();
}

#[tokio::test]
async fn cancel_during_manual_transcription_discards_the_text() {
    let (rec, _, tr, events, stop) = manual_stop_in_transcription().await;
    rec.cancel();
    assert_eq!(events.lock().unwrap().last(), Some(&RecorderState::Idle));
    tr.release.store(true, Ordering::SeqCst);
    assert_eq!(stop.await.unwrap(), Err(RecordError::NotRecording));
    assert_eq!(events.lock().unwrap().last(), Some(&RecorderState::Idle));
    assert_eq!(rec.stop().await, Err(RecordError::NotRecording));
}

type Partials = Arc<Mutex<Vec<(u64, String)>>>;

fn live_recorder(
    source: Arc<FakeSource>,
    tr: Arc<dyn Transcriber>,
    max: Duration,
    window: Duration,
) -> (Recorder, Events, Partials) {
    let events: Events = Arc::default();
    let partials: Partials = Arc::default();
    let (ev, pa) = (Arc::clone(&events), Arc::clone(&partials));
    let rec = Recorder::with_live(
        source,
        tr,
        max,
        Arc::new(move |s| ev.lock().unwrap().push(s)),
        Arc::new(move |r, t| pa.lock().unwrap().push((r, t))),
        LiveConfig {
            interval: Duration::from_millis(20),
            window,
        },
        tokio::runtime::Handle::current(),
    );
    (rec, events, partials)
}

async fn wait_partials(p: &Partials, n: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while p.lock().unwrap().len() < n {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("partial events");
}

/// Counts model loads and concurrent passes; text grows with every pass.
#[derive(Default)]
struct SessionTranscriber {
    opens: Arc<AtomicUsize>,
    closes: Arc<AtomicUsize>,
    running: Arc<AtomicUsize>,
    overlap: Arc<AtomicUsize>,
    passes: Arc<AtomicUsize>,
}
struct FakeSession(SessionTranscriber);
impl Transcriber for SessionTranscriber {
    fn transcribe(&self, _: &Path, _: &[f32], _: Option<&str>) -> Result<String, TranscribeError> {
        panic!("sessions only");
    }
    fn open_session(
        &self,
        _: &Path,
    ) -> Result<Option<Box<dyn TranscribeSession>>, TranscribeError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        Ok(Some(Box::new(FakeSession(SessionTranscriber {
            opens: Arc::clone(&self.opens),
            closes: Arc::clone(&self.closes),
            running: Arc::clone(&self.running),
            overlap: Arc::clone(&self.overlap),
            passes: Arc::clone(&self.passes),
        }))))
    }
}
impl TranscribeSession for FakeSession {
    fn transcribe(&self, samples: &[f32], _: Option<&str>) -> Result<String, TranscribeError> {
        if self.0.running.fetch_add(1, Ordering::SeqCst) > 0 {
            self.0.overlap.fetch_add(1, Ordering::SeqCst);
        }
        std::thread::sleep(Duration::from_millis(60)); // slower than the interval
        self.0.running.fetch_sub(1, Ordering::SeqCst);
        let n = self.0.passes.fetch_add(1, Ordering::SeqCst);
        Ok(format!("w{n}-{}", samples.len()))
    }
}
impl Drop for FakeSession {
    fn drop(&mut self) {
        self.0.closes.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn live_events_grow_before_stop_and_final_text_is_returned_once() {
    let tr = Arc::new(FakeTranscriber::default());
    let (rec, _, partials) = live_recorder(
        FakeSource::new(vec![0.1; 16_000]),
        Arc::clone(&tr) as Arc<dyn Transcriber>,
        MAX_RECORDING,
        LIVE_WINDOW,
    );
    rec.start(model(), None).unwrap();
    wait_partials(&partials, 1).await;
    // Same audio every pass: text is unchanged, so only one event so far.
    assert_eq!(partials.lock().unwrap()[0].1, "heard 16000 samples");
    assert_eq!(rec.stop().await.unwrap(), "heard 16000 samples");
}

#[tokio::test]
async fn live_revisions_increase_with_changing_text() {
    let tr = Arc::new(SessionTranscriber::default());
    let (rec, _, partials) = live_recorder(
        FakeSource::new(vec![0.1; 16_000]),
        tr,
        MAX_RECORDING,
        LIVE_WINDOW,
    );
    rec.start(model(), None).unwrap();
    wait_partials(&partials, 3).await;
    let seen = partials.lock().unwrap().clone();
    assert!(seen.windows(2).all(|w| w[0].0 < w[1].0), "{seen:?}");
    assert_ne!(seen[0].1, seen[1].1);
    rec.cancel();
}

#[tokio::test]
async fn one_pass_at_a_time_and_model_loaded_once_then_freed() {
    let tr = Arc::new(SessionTranscriber::default());
    let (opens, closes, overlap) = (
        Arc::clone(&tr.opens),
        Arc::clone(&tr.closes),
        Arc::clone(&tr.overlap),
    );
    let (rec, _, partials) = live_recorder(
        FakeSource::new(vec![0.1; 16_000]),
        tr,
        MAX_RECORDING,
        LIVE_WINDOW,
    );
    rec.start(model(), None).unwrap();
    wait_partials(&partials, 3).await;
    let text = rec.stop().await.unwrap();
    assert!(text.starts_with('w'));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        overlap.load(Ordering::SeqCst),
        0,
        "no overlapping live passes"
    );
    assert_eq!(opens.load(Ordering::SeqCst), 1, "model loaded once");
    assert_eq!(closes.load(Ordering::SeqCst), 1, "model freed after stop");
}

#[tokio::test]
async fn cancel_stops_live_events_and_frees_the_model() {
    let tr = Arc::new(SessionTranscriber::default());
    let closes = Arc::clone(&tr.closes);
    let (rec, _, partials) = live_recorder(
        FakeSource::new(vec![0.1; 16_000]),
        tr,
        MAX_RECORDING,
        LIVE_WINDOW,
    );
    rec.start(model(), None).unwrap();
    wait_partials(&partials, 1).await;
    rec.cancel();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let n = partials.lock().unwrap().len();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(partials.lock().unwrap().len(), n, "no event after cancel");
    assert_eq!(closes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn auto_stop_returns_final_text_after_live_events() {
    let tr = Arc::new(FakeTranscriber::default());
    let (rec, events, partials) = live_recorder(
        FakeSource::new(vec![0.1; 16_000]),
        tr,
        Duration::from_millis(300),
        LIVE_WINDOW,
    );
    rec.start(model(), None).unwrap();
    wait_partials(&partials, 1).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        while events.lock().unwrap().last() != Some(&RecorderState::Finished) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let n = partials.lock().unwrap().len();
    assert_eq!(rec.stop().await.unwrap(), "heard 16000 samples");
    assert_eq!(partials.lock().unwrap().len(), n);
}

#[tokio::test]
async fn long_audio_is_recognised_in_a_bounded_window() {
    // 3 s window, 3 s... of audio is 10 s: earlier audio is fixed in chunks.
    let tr = Arc::new(FakeTranscriber::default());
    let (rec, _, partials) = live_recorder(
        FakeSource::new(vec![0.1; 160_000]),
        Arc::clone(&tr) as Arc<dyn Transcriber>,
        MAX_RECORDING,
        Duration::from_secs(3),
    );
    rec.start(model(), None).unwrap();
    wait_partials(&partials, 1).await;
    assert!(tr.calls.lock().unwrap().iter().all(|c| c.1 <= 48_000));
    rec.cancel();
}

#[tokio::test]
async fn failed_final_pass_still_errors_after_live_text() {
    let tr = Arc::new(FakeTranscriber {
        fail: true,
        ..Default::default()
    });
    let (rec, _, _) = live_recorder(
        FakeSource::new(vec![0.1; 16_000]),
        tr,
        MAX_RECORDING,
        LIVE_WINDOW,
    );
    rec.start(model(), None).unwrap();
    assert_eq!(rec.stop().await, Err(RecordError::TranscriptionFailed));
}
