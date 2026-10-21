//! Local speech-to-text model management (Spec 0108): download a whisper.cpp
//! GGML model on explicit request, verify it against a pinned SHA-256, store
//! it, report its state, delete it; plus voice recording and local
//! transcription behind traits (`AudioSource`, `Transcriber`).
//!
//! Integrity rule: a model file only ever appears under its final name after
//! its hash was verified (download to `<name>.part`, verify, rename).
//! Logs carry at most the model id, a byte count and the outcome.

mod audio;
mod recorder;
mod transcribe;

pub use audio::{
    resample_mono, to_mono_16k, AudioCapture, AudioSource, CaptureError, CpalSource, TARGET_RATE,
};
pub use recorder::{
    LiveConfig, PartialCallback, RecordError, Recorder, RecorderState, StateCallback,
    LIVE_INTERVAL, LIVE_WINDOW, MAX_RECORDING,
};
pub use transcribe::{
    parse_language, InvalidLanguage, TranscribeError, TranscribeSession, Transcriber,
    WhisperTranscriber,
};

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;

/// Immutable revision of `ggerganov/whisper.cpp` the models are pinned to.
const REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

/// The supported models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechModel {
    Base,
    Small,
}

impl SpeechModel {
    pub const ALL: [SpeechModel; 2] = [SpeechModel::Base, SpeechModel::Small];
    pub const DEFAULT: SpeechModel = SpeechModel::Base;

    pub fn id(self) -> &'static str {
        match self {
            SpeechModel::Base => "base",
            SpeechModel::Small => "small",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    pub fn file_name(self) -> &'static str {
        match self {
            SpeechModel::Base => "ggml-base.bin",
            SpeechModel::Small => "ggml-small.bin",
        }
    }

    /// The single place that defines where a model comes from: fixed URL
    /// (pinned revision, not a branch), exact size, SHA-256.
    pub fn source(self) -> ModelSource {
        let (size, sha256) = match self {
            SpeechModel::Base => (
                147_951_465,
                "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
            ),
            SpeechModel::Small => (
                487_601_967,
                "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
            ),
        };
        ModelSource {
            url: format!(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/{REVISION}/{}",
                self.file_name()
            ),
            size,
            sha256: sha256.to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelSource {
    pub url: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

/// Translatable error keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelErrorKey {
    Network,
    HttpStatus,
    HashMismatch,
    Io,
}

impl ModelErrorKey {
    pub fn key(self) -> &'static str {
        match self {
            ModelErrorKey::Network => "speech_model_network",
            ModelErrorKey::HttpStatus => "speech_model_http_status",
            ModelErrorKey::HashMismatch => "speech_model_hash_mismatch",
            ModelErrorKey::Io => "speech_model_io",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ModelStatus {
    NotDownloaded,
    Downloading { done: u64, total: u64 },
    Ready,
    Error { key: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelManagerError {
    AlreadyDownloading,
    Downloading,
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DownloadProgress {
    pub done: u64,
    pub total: u64,
}

/// Called after each state change of a download: progress and the final
/// status (`Ready`, `Error`, `NotDownloaded` after cancel).
pub type StatusCallback = Arc<dyn Fn(SpeechModel, ModelStatus) + Send + Sync>;

enum Slot {
    Downloading {
        done: u64,
        total: u64,
        cancel: watch::Sender<bool>,
    },
    Error(ModelErrorKey),
}

pub struct ModelManager {
    dir: PathBuf,
    sources: HashMap<SpeechModel, ModelSource>,
    slots: Arc<Mutex<HashMap<SpeechModel, Slot>>>,
}

impl ModelManager {
    pub fn new(dir: PathBuf) -> Self {
        let sources = SpeechModel::ALL
            .into_iter()
            .map(|m| (m, m.source()))
            .collect();
        Self::with_sources(dir, sources)
    }

    /// Injected sources (tests use a local HTTP fixture).
    pub fn with_sources(dir: PathBuf, sources: HashMap<SpeechModel, ModelSource>) -> Self {
        Self {
            dir,
            sources,
            slots: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn model_path(&self, model: SpeechModel) -> PathBuf {
        self.dir.join(model.file_name())
    }

    fn part_path(&self, model: SpeechModel) -> PathBuf {
        part_path(&self.dir, model)
    }

    /// Pure local state; never touches the network.
    pub fn status(&self, model: SpeechModel) -> ModelStatus {
        match self.slots.lock().expect("slots lock").get(&model) {
            Some(Slot::Downloading { done, total, .. }) => ModelStatus::Downloading {
                done: *done,
                total: *total,
            },
            Some(Slot::Error(key)) => ModelStatus::Error { key: key.key() },
            None => {
                if self.model_path(model).is_file() {
                    ModelStatus::Ready
                } else {
                    ModelStatus::NotDownloaded
                }
            }
        }
    }

    /// Starts the download in the background and returns at once.
    pub fn start_download(
        &self,
        model: SpeechModel,
        on_status: StatusCallback,
    ) -> Result<(), ModelManagerError> {
        let source = self.sources.get(&model).cloned().expect("source per model");
        let (tx, rx) = watch::channel(false);
        {
            let mut slots = self.slots.lock().expect("slots lock");
            if matches!(slots.get(&model), Some(Slot::Downloading { .. })) {
                return Err(ModelManagerError::AlreadyDownloading);
            }
            slots.insert(
                model,
                Slot::Downloading {
                    done: 0,
                    total: source.size,
                    cancel: tx,
                },
            );
        }
        let dir = self.dir.clone();
        let slots = Arc::clone(&self.slots);
        tokio::spawn(async move {
            let result = download(&dir, model, &source, &slots, rx, &on_status).await;
            let final_status = {
                let mut guard = slots.lock().expect("slots lock");
                match result {
                    Ok(()) => {
                        guard.remove(&model);
                        ModelStatus::Ready
                    }
                    Err(None) => {
                        guard.remove(&model);
                        ModelStatus::NotDownloaded
                    }
                    Err(Some(key)) => {
                        guard.insert(model, Slot::Error(key));
                        ModelStatus::Error { key: key.key() }
                    }
                }
            };
            tracing::info!(model = model.id(), status = ?final_status, "speech model download finished");
            on_status(model, final_status);
        });
        Ok(())
    }

    /// Cancels a running download; returns whether one was running. The task
    /// removes the temp file and reports `NotDownloaded`.
    pub fn cancel_download(&self, model: SpeechModel) -> bool {
        if let Some(Slot::Downloading { cancel, .. }) =
            self.slots.lock().expect("slots lock").get(&model)
        {
            let _ = cancel.send(true);
            true
        } else {
            false
        }
    }

    /// Removes the model file (and any leftover temp file) and clears a
    /// recorded error. Refused while a download runs.
    pub fn delete(&self, model: SpeechModel) -> Result<(), ModelManagerError> {
        let mut slots = self.slots.lock().expect("slots lock");
        if matches!(slots.get(&model), Some(Slot::Downloading { .. })) {
            return Err(ModelManagerError::Downloading);
        }
        for path in [self.model_path(model), self.part_path(model)] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(ModelManagerError::Io),
            }
        }
        slots.remove(&model);
        tracing::info!(model = model.id(), "speech model deleted");
        Ok(())
    }
}

fn part_path(dir: &Path, model: SpeechModel) -> PathBuf {
    dir.join(format!("{}.part", model.file_name()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `Err(None)` = cancelled, `Err(Some(key))` = failed. The temp file is gone
/// in every error case.
async fn download(
    dir: &Path,
    model: SpeechModel,
    source: &ModelSource,
    slots: &Mutex<HashMap<SpeechModel, Slot>>,
    mut cancel: watch::Receiver<bool>,
    on_status: &StatusCallback,
) -> Result<(), Option<ModelErrorKey>> {
    let part = part_path(dir, model);
    let result = download_inner(dir, model, source, &part, slots, &mut cancel, on_status).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&part).await;
    }
    result
}

async fn download_inner(
    dir: &Path,
    model: SpeechModel,
    source: &ModelSource,
    part: &Path,
    slots: &Mutex<HashMap<SpeechModel, Slot>>,
    cancel: &mut watch::Receiver<bool>,
    on_status: &StatusCallback,
) -> Result<(), Option<ModelErrorKey>> {
    let io = |_| Some(ModelErrorKey::Io);
    tokio::fs::create_dir_all(dir).await.map_err(io)?;
    let _ = tokio::fs::remove_file(part).await;

    let response = tokio::select! {
        _ = cancel.changed() => return Err(None),
        r = reqwest::Client::new().get(&source.url).send() => r,
    }
    .map_err(|_| Some(ModelErrorKey::Network))?;
    if !response.status().is_success() {
        return Err(Some(ModelErrorKey::HttpStatus));
    }

    let mut file = tokio::fs::File::create(part).await.map_err(io)?;
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    let mut stream = response.bytes_stream();
    loop {
        let chunk = tokio::select! {
            _ = cancel.changed() => return Err(None),
            c = stream.next() => c,
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(|_| Some(ModelErrorKey::Network))?;
        done += chunk.len() as u64;
        // A body larger than the pinned size can never match the hash.
        if done > source.size {
            return Err(Some(ModelErrorKey::HashMismatch));
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(io)?;
        if let Some(Slot::Downloading { done: d, .. }) =
            slots.lock().expect("slots lock").get_mut(&model)
        {
            *d = done;
        }
        on_status(
            model,
            ModelStatus::Downloading {
                done,
                total: source.size,
            },
        );
    }
    file.flush().await.map_err(io)?;
    file.sync_all().await.map_err(io)?;
    drop(file);

    if done != source.size || hex(&hasher.finalize()) != source.sha256 {
        return Err(Some(ModelErrorKey::HashMismatch));
    }
    if *cancel.borrow() {
        return Err(None);
    }
    tokio::fs::rename(part, dir.join(model.file_name()))
        .await
        .map_err(io)?;
    Ok(())
}

#[cfg(test)]
mod tests;
