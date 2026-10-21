//! Recording state machine (Spec 0108, section 5): one recording at a time,
//! hard maximum length, transcription off the caller's thread. Audio only
//! lives in memory and is dropped after transcription or cancel.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::audio::{AudioCapture, AudioSource, CaptureError, TARGET_RATE};
use crate::transcribe::{TranscribeError, TranscribeSession, Transcriber};

/// Default hard limit of one recording.
pub const MAX_RECORDING: Duration = Duration::from_secs(120);

/// Delay between two live recognition passes.
pub const LIVE_INTERVAL: Duration = Duration::from_secs(1);
/// Longest stretch of audio one live pass recognises; older audio is
/// recognised once in chunks and its text stays fixed.
pub const LIVE_WINDOW: Duration = Duration::from_secs(25);

/// Live recognition settings.
#[derive(Debug, Clone, Copy)]
pub struct LiveConfig {
    pub interval: Duration,
    pub window: Duration,
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            interval: LIVE_INTERVAL,
            window: LIVE_WINDOW,
        }
    }
}

/// Provisional text: `(revision, full provisional text so far)`. Revisions
/// only grow, across recordings too.
pub type PartialCallback = Arc<dyn Fn(u64, String) + Send + Sync>;

/// One model instance for the passes of one recording.
enum Engine {
    Session(Box<dyn TranscribeSession>),
    PerCall(Arc<dyn Transcriber>, PathBuf),
}

impl Engine {
    fn open(transcriber: &Arc<dyn Transcriber>, model: &Path) -> Result<Self, TranscribeError> {
        Ok(match transcriber.open_session(model)? {
            Some(s) => Engine::Session(s),
            None => Engine::PerCall(Arc::clone(transcriber), model.to_path_buf()),
        })
    }

    fn run(&self, samples: &[f32], language: Option<&str>) -> Result<String, TranscribeError> {
        match self {
            Engine::Session(s) => s.transcribe(samples, language),
            Engine::PerCall(t, m) => t.transcribe(m, samples, language),
        }
    }
}

type EngineSlot = Arc<Mutex<Option<Arc<Engine>>>>;

fn engine_from(
    slot: &EngineSlot,
    transcriber: &Arc<dyn Transcriber>,
    model: &Path,
) -> Result<Arc<Engine>, TranscribeError> {
    let mut g = slot.lock().map_err(|_| TranscribeError)?;
    if let Some(e) = &*g {
        return Ok(Arc::clone(e));
    }
    let e = Arc::new(Engine::open(transcriber, model)?);
    *g = Some(Arc::clone(&e));
    Ok(e)
}

fn join_text(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_owned(),
        (_, true) => a.to_owned(),
        _ => format!("{a} {b}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordError {
    NoModel,
    NoDevice,
    PermissionDenied,
    TranscriptionFailed,
    AlreadyRecording,
    NotRecording,
}

impl RecordError {
    /// Translatable error key.
    pub fn key(self) -> &'static str {
        match self {
            RecordError::NoModel => "speech_no_model",
            RecordError::NoDevice => "speech_no_device",
            RecordError::PermissionDenied => "speech_permission_denied",
            RecordError::TranscriptionFailed => "speech_transcription_failed",
            RecordError::AlreadyRecording => "speech_already_recording",
            RecordError::NotRecording => "speech_not_recording",
        }
    }
}

impl From<CaptureError> for RecordError {
    fn from(e: CaptureError) -> Self {
        match e {
            CaptureError::NoDevice => RecordError::NoDevice,
            CaptureError::PermissionDenied => RecordError::PermissionDenied,
            CaptureError::Failed => RecordError::NoDevice,
        }
    }
}

/// State changes for the frontend. `Finished` only follows an automatic stop
/// at the maximum length: the result waits for the next `stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecorderState {
    Recording,
    Transcribing,
    Idle,
    Finished,
}

pub type StateCallback = Arc<dyn Fn(RecorderState) + Send + Sync>;

type Outcome = Result<String, RecordError>;

struct Active {
    id: u64,
    capture: Box<dyn AudioCapture>,
    model: PathBuf,
    language: Option<String>,
    started: Instant,
    timer: JoinHandle<()>,
    live: JoinHandle<()>,
    engine: EngineSlot,
}

enum State {
    Idle,
    Recording(Active),
    Transcribing {
        id: u64,
        done: watch::Receiver<Option<Outcome>>,
    },
    Finished(Outcome),
}

struct Inner {
    source: Arc<dyn AudioSource>,
    transcriber: Arc<dyn Transcriber>,
    max: Duration,
    on_state: StateCallback,
    on_partial: PartialCallback,
    live: LiveConfig,
    revision: AtomicU64,
    runtime: tokio::runtime::Handle,
    state: Mutex<State>,
    next_id: Mutex<u64>,
}

#[derive(Clone)]
pub struct Recorder(Arc<Inner>);

impl Recorder {
    pub fn new(
        source: Arc<dyn AudioSource>,
        transcriber: Arc<dyn Transcriber>,
        max: Duration,
        on_state: StateCallback,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Self::with_live(
            source,
            transcriber,
            max,
            on_state,
            Arc::new(|_, _| {}),
            LiveConfig::default(),
            runtime,
        )
    }

    /// Like `new`, plus live provisional text while recording.
    pub fn with_live(
        source: Arc<dyn AudioSource>,
        transcriber: Arc<dyn Transcriber>,
        max: Duration,
        on_state: StateCallback,
        on_partial: PartialCallback,
        live: LiveConfig,
        runtime: tokio::runtime::Handle,
    ) -> Self {
        Recorder(Arc::new(Inner {
            source,
            transcriber,
            max,
            on_state,
            on_partial,
            live,
            revision: AtomicU64::new(0),
            runtime,
            state: Mutex::new(State::Idle),
            next_id: Mutex::new(0),
        }))
    }

    /// Opens the device and starts capturing. `model` is the verified model
    /// file, or `None` when no model is ready: then the device stays closed.
    /// `language` is `None` for auto-detection.
    pub fn start(
        &self,
        model: Option<PathBuf>,
        language: Option<String>,
    ) -> Result<(), RecordError> {
        let inner = &self.0;
        let mut state = inner.state.lock().expect("recorder state");
        if matches!(&*state, State::Recording(_) | State::Transcribing { .. }) {
            return Err(RecordError::AlreadyRecording);
        }
        let model = model.ok_or(RecordError::NoModel)?;
        let max_samples = (inner.max.as_secs_f64() * TARGET_RATE as f64) as usize;
        let capture = inner.source.open(max_samples).map_err(RecordError::from)?;
        let id = {
            let mut n = inner.next_id.lock().expect("recorder id");
            *n += 1;
            *n
        };
        let this = self.clone();
        let max = inner.max;
        let timer = inner.runtime.spawn(async move {
            tokio::time::sleep(max).await;
            this.auto_stop(id).await;
        });
        let engine: EngineSlot = Arc::new(Mutex::new(None));
        let this = self.clone();
        let (live_model, live_language, live_engine) =
            (model.clone(), language.clone(), Arc::clone(&engine));
        let live = inner.runtime.spawn(async move {
            this.live_loop(id, live_model, live_language, live_engine)
                .await;
        });
        *state = State::Recording(Active {
            id,
            capture,
            model,
            language,
            started: Instant::now(),
            timer,
            live,
            engine,
        });
        drop(state);
        (inner.on_state)(RecorderState::Recording);
        tracing::info!("voice recording started");
        Ok(())
    }

    /// Stops the recording and returns the transcribed text. After an
    /// automatic stop it returns that recording's result.
    pub async fn stop(&self) -> Result<String, RecordError> {
        let inner = &self.0;
        let (tx, rx) = watch::channel(None);
        let taken = {
            let mut state = inner.state.lock().expect("recorder state");
            match std::mem::replace(&mut *state, State::Idle) {
                State::Recording(active) => {
                    // Stay busy while recognising: start is refused, cancel
                    // can discard the result.
                    *state = State::Transcribing {
                        id: active.id,
                        done: rx,
                    };
                    Ok(active)
                }
                State::Finished(outcome) => {
                    drop(state);
                    (inner.on_state)(RecorderState::Idle);
                    return outcome;
                }
                State::Transcribing { id, done } => {
                    *state = State::Transcribing {
                        id,
                        done: done.clone(),
                    };
                    Err((id, done))
                }
                State::Idle => return Err(RecordError::NotRecording),
            }
        };
        match taken {
            Ok(active) => {
                active.timer.abort();
                (inner.on_state)(RecorderState::Transcribing);
                let id = active.id;
                let outcome = self.transcribe(active).await;
                let _ = tx.send(Some(outcome.clone()));
                let mut state = inner.state.lock().expect("recorder state");
                if matches!(&*state, State::Transcribing { id: i, .. } if *i == id) {
                    *state = State::Idle;
                    drop(state);
                    (inner.on_state)(RecorderState::Idle);
                    outcome
                } else {
                    // Cancelled meanwhile (cancel already reported Idle):
                    // the result is discarded.
                    Err(RecordError::NotRecording)
                }
            }
            Err((id, mut done)) => {
                // Automatic stop already transcribing: wait for its result.
                let outcome = loop {
                    if let Some(o) = done.borrow().clone() {
                        break o;
                    }
                    if done.changed().await.is_err() {
                        return Err(RecordError::NotRecording);
                    }
                };
                let mut state = inner.state.lock().expect("recorder state");
                if matches!(&*state, State::Finished(_))
                    || matches!(&*state, State::Transcribing { id: i, .. } if *i == id)
                {
                    *state = State::Idle;
                }
                drop(state);
                (inner.on_state)(RecorderState::Idle);
                outcome
            }
        }
    }

    /// Discards the recording; nothing is transcribed or returned.
    pub fn cancel(&self) {
        let inner = &self.0;
        let old = std::mem::replace(
            &mut *inner.state.lock().expect("recorder state"),
            State::Idle,
        );
        if let State::Recording(active) = old {
            active.timer.abort();
            active.live.abort();
            drop(active.capture); // stops the device, drops the audio
            tracing::info!("voice recording cancelled");
        }
        (inner.on_state)(RecorderState::Idle);
    }

    async fn auto_stop(&self, id: u64) {
        let inner = &self.0;
        let (tx, rx) = watch::channel(None);
        let active = {
            let mut state = inner.state.lock().expect("recorder state");
            match &*state {
                State::Recording(a) if a.id == id => {}
                _ => return,
            }
            let State::Recording(a) =
                std::mem::replace(&mut *state, State::Transcribing { id, done: rx })
            else {
                return;
            };
            a
        };
        tracing::info!("voice recording reached the maximum length");
        (inner.on_state)(RecorderState::Transcribing);
        let outcome = self.transcribe(active).await;
        {
            let mut state = inner.state.lock().expect("recorder state");
            if matches!(&*state, State::Transcribing { id: i, .. } if *i == id) {
                *state = State::Finished(outcome.clone());
            } else {
                // cancelled or already collected meanwhile
                let _ = tx.send(Some(outcome));
                return;
            }
        }
        let _ = tx.send(Some(outcome));
        (inner.on_state)(RecorderState::Finished);
    }

    /// Recognises the recording so far every `interval` and reports the
    /// provisional text. At most one pass is in flight: the next one starts
    /// after the previous finished. Audio older than the window is
    /// recognised once in chunks and kept as fixed text.
    async fn live_loop(&self, id: u64, model: PathBuf, language: Option<String>, slot: EngineSlot) {
        let inner = &self.0;
        let window = (inner.live.window.as_secs_f64() * TARGET_RATE as f64) as usize;
        let chunk = window / 5 * 4;
        let mut fixed = String::new();
        let mut offset = 0usize;
        let mut last = String::new();
        loop {
            tokio::time::sleep(inner.live.interval).await;
            let samples = {
                let state = inner.state.lock().expect("recorder state");
                match &*state {
                    State::Recording(a) if a.id == id => a.capture.snapshot(),
                    _ => return,
                }
            };
            // Less than half a second of new audio: nothing to recognise.
            if samples.len() < offset + (TARGET_RATE / 2) as usize {
                continue;
            }
            let transcriber = Arc::clone(&inner.transcriber);
            let (m, l, sl) = (model.clone(), language.clone(), Arc::clone(&slot));
            let (f_in, o_in) = (std::mem::take(&mut fixed), offset);
            let started = Instant::now();
            let res = tokio::task::spawn_blocking(move || {
                let mut fixed = f_in;
                let mut offset = o_in;
                let engine = match engine_from(&sl, &transcriber, &m) {
                    Ok(e) => e,
                    Err(_) => return Err((fixed, offset)),
                };
                while window > 0 && samples.len() - offset > window {
                    match engine.run(&samples[offset..offset + chunk], l.as_deref()) {
                        Ok(t) => {
                            fixed = join_text(&fixed, &t);
                            offset += chunk;
                        }
                        Err(_) => return Err((fixed, offset)),
                    }
                }
                match engine.run(&samples[offset..], l.as_deref()) {
                    Ok(t) => Ok((join_text(&fixed, &t), fixed, offset)),
                    Err(_) => Err((fixed, offset)),
                }
            })
            .await;
            let text = match res {
                Ok(Ok((text, f, o))) => {
                    fixed = f;
                    offset = o;
                    text
                }
                Ok(Err((f, o))) => {
                    fixed = f;
                    offset = o;
                    continue;
                }
                Err(_) => return,
            };
            tracing::debug!(
                chars = text.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "live voice pass finished"
            );
            if text == last {
                continue;
            }
            // Report only while this recording is still the running one.
            let state = inner.state.lock().expect("recorder state");
            if !matches!(&*state, State::Recording(a) if a.id == id) {
                return;
            }
            let rev = inner.revision.fetch_add(1, Ordering::SeqCst) + 1;
            (inner.on_partial)(rev, text.clone());
            drop(state);
            last = text;
        }
    }

    async fn transcribe(&self, active: Active) -> Outcome {
        let transcriber = Arc::clone(&self.0.transcriber);
        let Active {
            capture,
            model,
            language,
            started,
            live,
            engine,
            ..
        } = active;
        live.abort();
        let result = tokio::task::spawn_blocking(move || {
            let samples = capture.finish(); // dropped at the end of this closure
            let secs = samples.len() as f32 / TARGET_RATE as f32;
            // Less than a tenth of a second: nothing to recognise.
            if samples.len() < (TARGET_RATE / 10) as usize {
                return (secs, Ok(String::new()));
            }
            let text = engine_from(&engine, &transcriber, &model)
                .and_then(|e| e.run(&samples, language.as_deref()))
                .map_err(|_| RecordError::TranscriptionFailed);
            (secs, text)
        })
        .await;
        match result {
            Ok((secs, outcome)) => {
                tracing::info!(
                    audio_seconds = secs,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    ok = outcome.is_ok(),
                    "voice transcription finished"
                );
                outcome
            }
            Err(_) => Err(RecordError::TranscriptionFailed),
        }
    }
}

#[cfg(test)]
mod tests;
