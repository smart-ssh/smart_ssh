//! Local speech-to-text model management (Spec 0108): download a whisper.cpp
//! GGML model on explicit request, verify it against a pinned SHA-256, store
//! it, report its state, delete it; plus voice recording and local
//! transcription behind traits (`AudioSource`, `Transcriber`).
//!
//! Integrity rule: a model file only ever appears under its final name after
//! its hash was verified (download to `<name>.part`, verify, rename). A model
//! of several files is ready only when every file was verified; on cancel or
//! failure nothing of it remains.
//! Logs carry at most the model id, a byte count and the outcome.

mod audio;
mod parakeet;
mod recorder;
mod transcribe;

pub use audio::{resample_mono, AudioCapture, AudioSource, CaptureError, CpalSource, TARGET_RATE};
pub use parakeet::ParakeetTranscriber;
pub use recorder::{
    LiveConfig, PartialCallback, RecordError, Recorder, RecorderState, StateCallback,
    LIVE_INTERVAL, LIVE_WINDOW, MAX_RECORDING,
};
pub use transcribe::{
    parse_language, InvalidLanguage, SpeechTranscriber, TranscribeError, TranscribeSession,
    Transcriber, WhisperTranscriber,
};

use ssh_manager_core::sync::lock_tolerating_poison;
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

/// Immutable revision of `istupakov/parakeet-tdt-0.6b-v3-onnx` (CC-BY-4.0,
/// conversion of NVIDIA's Parakeet TDT 0.6B v3) the Parakeet files are pinned to.
const PARAKEET_REVISION: &str = "8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce";

/// The supported models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechModel {
    Base,
    Small,
    ParakeetV3,
}

/// The recognition engine a model runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Whisper,
    Parakeet,
}

impl SpeechModel {
    /// Every model the crate knows; see [`SpeechModel::available`] for the
    /// ones offered on the current platform.
    pub const ALL: [SpeechModel; 3] = [
        SpeechModel::Base,
        SpeechModel::Small,
        SpeechModel::ParakeetV3,
    ];
    pub const DEFAULT: SpeechModel = SpeechModel::Base;

    /// Models offered on this platform (all of them, Android included).
    pub fn available() -> Vec<SpeechModel> {
        Self::ALL.into_iter().filter(|m| m.is_available()).collect()
    }

    pub fn is_available(self) -> bool {
        match self.engine() {
            Engine::Whisper | Engine::Parakeet => true,
        }
    }

    pub fn engine(self) -> Engine {
        match self {
            SpeechModel::Base | SpeechModel::Small => Engine::Whisper,
            SpeechModel::ParakeetV3 => Engine::Parakeet,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            SpeechModel::Base => "base",
            SpeechModel::Small => "small",
            SpeechModel::ParakeetV3 => "parakeet_v3",
        }
    }

    /// `None` for an unknown id and for a model this platform does not offer.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|m| m.id() == id && m.is_available())
    }

    /// File name of a single-file model, directory name of a multi-file one.
    pub fn file_name(self) -> &'static str {
        match self {
            SpeechModel::Base => "ggml-base.bin",
            SpeechModel::Small => "ggml-small.bin",
            SpeechModel::ParakeetV3 => "parakeet-tdt-0.6b-v3-int8",
        }
    }

    /// Whether the model is a directory of several files.
    pub fn is_multi_file(self) -> bool {
        matches!(self, SpeechModel::ParakeetV3)
    }

    /// The single place that defines where a model comes from: fixed URLs
    /// (pinned revision, not a branch), exact sizes, SHA-256 per file.
    pub fn source(self) -> ModelSource {
        let whisper = |size, sha256: &str| ModelSource {
            files: vec![ModelFile {
                name: self.file_name().to_owned(),
                url: format!(
                    "https://huggingface.co/{}/resolve/{REVISION}/{}",
                    self.source_repo(),
                    self.file_name()
                ),
                size,
                sha256: sha256.to_owned(),
            }],
        };
        match self {
            SpeechModel::Base => whisper(
                147_951_465,
                "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
            ),
            SpeechModel::Small => whisper(
                487_601_967,
                "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
            ),
            SpeechModel::ParakeetV3 => {
                let file = |name: &str, size, sha256: &str| ModelFile {
                    name: name.to_owned(),
                    url: format!(
                        "https://huggingface.co/{}/resolve/{PARAKEET_REVISION}/{name}",
                        self.source_repo()
                    ),
                    size,
                    sha256: sha256.to_owned(),
                };
                ModelSource {
                    files: vec![
                        file(
                            "encoder-model.int8.onnx",
                            652_183_999,
                            "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09",
                        ),
                        file(
                            "decoder_joint-model.int8.onnx",
                            18_202_004,
                            "eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70",
                        ),
                        file(
                            "nemo128.onnx",
                            139_764,
                            "a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f",
                        ),
                        file(
                            "vocab.txt",
                            93_939,
                            "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
                        ),
                    ],
                }
            }
        }
    }
}

/// What the UI needs to list a model without hard-coding it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelInfo {
    pub id: &'static str,
    pub engine: Engine,
    /// Sum of all files, in bytes.
    pub total_size: u64,
    pub source_host: &'static str,
    pub source_repo: &'static str,
}

impl SpeechModel {
    /// Hugging Face repository the files come from.
    pub fn source_repo(self) -> &'static str {
        match self.engine() {
            Engine::Whisper => "ggerganov/whisper.cpp",
            Engine::Parakeet => "istupakov/parakeet-tdt-0.6b-v3-onnx",
        }
    }

    pub fn info(self) -> ModelInfo {
        ModelInfo {
            id: self.id(),
            engine: self.engine(),
            total_size: self.source().total_size(),
            source_host: "huggingface.co",
            source_repo: self.source_repo(),
        }
    }

    /// The models of this platform, with their details. Pure; no network.
    pub fn available_info() -> Vec<ModelInfo> {
        Self::available().into_iter().map(Self::info).collect()
    }
}

/// One downloadable file of a model.
#[derive(Debug, Clone)]
pub struct ModelFile {
    /// File name inside the model's directory.
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

/// All files of a model; the model is usable only when every one was verified.
#[derive(Debug, Clone)]
pub struct ModelSource {
    pub files: Vec<ModelFile>,
}

impl ModelSource {
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
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

    /// The file (single-file model) or directory (multi-file model) the
    /// transcriber loads.
    pub fn model_path(&self, model: SpeechModel) -> PathBuf {
        self.dir.join(model.file_name())
    }

    /// The injected source, else the pinned one.
    fn source_of(&self, model: SpeechModel) -> ModelSource {
        self.sources
            .get(&model)
            .cloned()
            .unwrap_or_else(|| model.source())
    }

    /// Pure local state; never touches the network.
    pub fn status(&self, model: SpeechModel) -> ModelStatus {
        match lock_tolerating_poison(&self.slots).get(&model) {
            Some(Slot::Downloading { done, total, .. }) => ModelStatus::Downloading {
                done: *done,
                total: *total,
            },
            Some(Slot::Error(key)) => ModelStatus::Error { key: key.key() },
            None => {
                if files_complete(&self.dir, model, &self.source_of(model)) {
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
        let source = self.source_of(model);
        let (tx, rx) = watch::channel(false);
        {
            let mut slots = lock_tolerating_poison(&self.slots);
            if matches!(slots.get(&model), Some(Slot::Downloading { .. })) {
                return Err(ModelManagerError::AlreadyDownloading);
            }
            slots.insert(
                model,
                Slot::Downloading {
                    done: 0,
                    total: source.total_size(),
                    cancel: tx,
                },
            );
        }
        let dir = self.dir.clone();
        let slots = Arc::clone(&self.slots);
        tokio::spawn(async move {
            let result = download(&dir, model, &source, &slots, rx, &on_status).await;
            let final_status = {
                let mut guard = lock_tolerating_poison(&slots);
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
            lock_tolerating_poison(&self.slots).get(&model)
        {
            let _ = cancel.send(true);
            true
        } else {
            false
        }
    }

    /// Removes every file of the model (and any leftover temp file) and clears a
    /// recorded error. Refused while a download runs.
    pub fn delete(&self, model: SpeechModel) -> Result<(), ModelManagerError> {
        let mut slots = lock_tolerating_poison(&self.slots);
        if matches!(slots.get(&model), Some(Slot::Downloading { .. })) {
            return Err(ModelManagerError::Downloading);
        }
        if remove_model_files(&self.dir, model, &self.source_of(model)).is_err() {
            return Err(ModelManagerError::Io);
        }
        slots.remove(&model);
        tracing::info!(model = model.id(), "speech model deleted");
        Ok(())
    }
}

/// Directory the files of a model live in: the model's own directory for a
/// multi-file model, the shared models directory for a single-file one.
fn files_dir(dir: &Path, model: SpeechModel) -> PathBuf {
    if model.is_multi_file() {
        dir.join(model.file_name())
    } else {
        dir.to_path_buf()
    }
}

fn part_path(dir: &Path, model: SpeechModel, file: &ModelFile) -> PathBuf {
    files_dir(dir, model).join(format!("{}.part", file.name))
}

fn final_path(dir: &Path, model: SpeechModel, file: &ModelFile) -> PathBuf {
    files_dir(dir, model).join(&file.name)
}

fn files_complete(dir: &Path, model: SpeechModel, source: &ModelSource) -> bool {
    source
        .files
        .iter()
        .all(|f| final_path(dir, model, f).is_file())
}

/// Removes every final and temporary file of the model, then its directory
/// when it is a multi-file model. A missing file is fine.
fn remove_model_files(dir: &Path, model: SpeechModel, source: &ModelSource) -> std::io::Result<()> {
    for f in &source.files {
        for path in [final_path(dir, model, f), part_path(dir, model, f)] {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
    }
    if model.is_multi_file() {
        match std::fs::remove_dir(files_dir(dir, model)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            // Something foreign lives there: leave it.
            Err(_) => {}
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `Err(None)` = cancelled, `Err(Some(key))` = failed. No temp file of the
/// model remains in any error case, and a failed download never leaves a
/// final file behind.
async fn download(
    dir: &Path,
    model: SpeechModel,
    source: &ModelSource,
    slots: &Mutex<HashMap<SpeechModel, Slot>>,
    mut cancel: watch::Receiver<bool>,
    on_status: &StatusCallback,
) -> Result<(), Option<ModelErrorKey>> {
    let result = download_all(dir, model, source, slots, &mut cancel, on_status).await;
    if result.is_err() {
        for f in &source.files {
            let _ = tokio::fs::remove_file(part_path(dir, model, f)).await;
        }
        if model.is_multi_file() {
            // Only removes the directory when it is empty.
            let _ = tokio::fs::remove_dir(files_dir(dir, model)).await;
        }
    }
    result
}

/// Downloads every file to `<name>.part` and verifies it; only when all of
/// them matched, the files are renamed to their final names.
async fn download_all(
    dir: &Path,
    model: SpeechModel,
    source: &ModelSource,
    slots: &Mutex<HashMap<SpeechModel, Slot>>,
    cancel: &mut watch::Receiver<bool>,
    on_status: &StatusCallback,
) -> Result<(), Option<ModelErrorKey>> {
    let io = |_| Some(ModelErrorKey::Io);
    tokio::fs::create_dir_all(files_dir(dir, model))
        .await
        .map_err(io)?;
    let client = download_client().map_err(|e| {
        tracing::error!(error = %e, "failed to build the model download HTTP client");
        Some(ModelErrorKey::Network)
    })?;
    let total = source.total_size();
    let mut base: u64 = 0;
    for file in &source.files {
        download_file(
            &client,
            model,
            file,
            &part_path(dir, model, file),
            (base, total),
            slots,
            cancel,
            on_status,
        )
        .await?;
        base += file.size;
    }
    if *cancel.borrow() {
        return Err(None);
    }
    for (i, file) in source.files.iter().enumerate() {
        if let Err(e) =
            tokio::fs::rename(part_path(dir, model, file), final_path(dir, model, file)).await
        {
            // Never leave a half-renamed model behind.
            for done in &source.files[..i] {
                let _ = tokio::fs::remove_file(final_path(dir, model, done)).await;
            }
            return Err(io(e));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn download_file(
    client: &reqwest::Client,
    model: SpeechModel,
    file: &ModelFile,
    part: &Path,
    (base, total): (u64, u64),
    slots: &Mutex<HashMap<SpeechModel, Slot>>,
    cancel: &mut watch::Receiver<bool>,
    on_status: &StatusCallback,
) -> Result<(), Option<ModelErrorKey>> {
    let io = |_| Some(ModelErrorKey::Io);
    let _ = tokio::fs::remove_file(part).await;
    let response = tokio::select! {
        _ = cancel.changed() => return Err(None),
        r = client.get(&file.url).send() => r,
    }
    .map_err(|_| Some(ModelErrorKey::Network))?;
    if !response.status().is_success() {
        return Err(Some(ModelErrorKey::HttpStatus));
    }

    let mut out = tokio::fs::File::create(part).await.map_err(io)?;
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
        if done > file.size {
            return Err(Some(ModelErrorKey::HashMismatch));
        }
        hasher.update(&chunk);
        out.write_all(&chunk).await.map_err(io)?;
        if let Some(Slot::Downloading { done: d, .. }) =
            lock_tolerating_poison(slots).get_mut(&model)
        {
            *d = base + done;
        }
        on_status(
            model,
            ModelStatus::Downloading {
                done: base + done,
                total,
            },
        );
    }
    out.flush().await.map_err(io)?;
    out.sync_all().await.map_err(io)?;
    drop(out);

    if done != file.size || hex(&hasher.finalize()) != file.sha256 {
        return Err(Some(ModelErrorKey::HashMismatch));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

/// HTTP client for model downloads. On Android the platform certificate
/// verifier would need JNI initialisation, so the bundled webpki roots are
/// used there (Spec 0114, ADR 0148).
fn download_client() -> Result<reqwest::Client, reqwest::Error> {
    let builder = reqwest::Client::builder();
    #[cfg(target_os = "android")]
    let builder = builder.tls_backend_preconfigured(tls_config::webpki_client_config());
    match builder.build() {
        Ok(client) => Ok(client),
        // Android must not fall back to `Client::new()`: that would use the
        // platform verifier, which needs JNI initialisation (ADR 0148).
        #[cfg(target_os = "android")]
        Err(e) => Err(e),
        #[cfg(not(target_os = "android"))]
        Err(_) => Ok(reqwest::Client::new()),
    }
}
