//! Local speech-to-text model management (Spec 0108): download an ONNX speech
//! model on explicit request, verify it against a pinned SHA-256, store
//! it, report its state, delete it; plus voice recording and local
//! transcription behind traits (`AudioSource`, `Transcriber`).
//!
//! Integrity rule: a model file only ever appears under its final name after
//! its hash was verified (download to `<name>.part`, verify, rename). A model
//! (several files) is ready only when every file was verified; on cancel or
//! failure nothing of it remains.
//! Logs carry at most the model id, a byte count and the outcome.

mod audio;
mod parakeet;
mod recorder;
mod transcribe;
mod whisper;

pub use audio::{resample_mono, AudioCapture, AudioSource, CaptureError, CpalSource, TARGET_RATE};
pub use parakeet::ParakeetTranscriber;
pub use recorder::{
    LiveConfig, PartialCallback, RecordError, Recorder, RecorderState, StateCallback,
    LIVE_INTERVAL, LIVE_WINDOW, MAX_RECORDING,
};
pub use transcribe::{
    parse_language, InvalidLanguage, SpeechTranscriber, TranscribeError, TranscribeSession,
    Transcriber,
};
pub use whisper::WhisperTranscriber;

use ssh_manager_core::sync::lock_tolerating_poison;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;

/// Immutable revisions of `onnx-community/whisper-base` / `-small` (Apache-2.0,
/// int8 ONNX export of OpenAI Whisper) the Whisper files are pinned to.
const WHISPER_BASE_REVISION: &str = "27a07b4b52b3a4c5886134bb4320e93e1a4bbc7c";
const WHISPER_SMALL_REVISION: &str = "36050c46d777d46dc4b5f43f6d90574fc38f8732";

/// Whisper files of the old whisper.cpp format; removed on start
/// ([`remove_legacy_files`]).
const LEGACY_FILES: [&str; 2] = ["ggml-base.bin", "ggml-small.bin"];

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

    /// Name of the model's directory inside the models directory.
    pub fn file_name(self) -> &'static str {
        match self {
            SpeechModel::Base => "whisper-base-int8",
            SpeechModel::Small => "whisper-small-int8",
            SpeechModel::ParakeetV3 => "parakeet-tdt-0.6b-v3-int8",
        }
    }

    /// The single place that defines where a model comes from: fixed URLs
    /// (pinned revision, not a branch), exact sizes, SHA-256 per file.
    pub fn source(self) -> ModelSource {
        let whisper = |revision: &str, files: [(&str, &str, u64, &str); 5]| ModelSource {
            files: files
                .into_iter()
                .map(|(name, remote, size, sha256)| ModelFile {
                    name: name.to_owned(),
                    url: format!(
                        "https://huggingface.co/{}/resolve/{revision}/{remote}",
                        self.source_repo()
                    ),
                    size,
                    sha256: sha256.to_owned(),
                })
                .collect(),
        };
        match self {
            SpeechModel::Base => whisper(
                WHISPER_BASE_REVISION,
                [
                    (
                        "encoder_model_int8.onnx",
                        "onnx/encoder_model_int8.onnx",
                        23_201_297,
                        "ca6177401f86a2c6b4dc5f7fc02fbca680678906bd0c22f6d89f0b80f124253f",
                    ),
                    (
                        "decoder_model_merged_int8.onnx",
                        "onnx/decoder_model_merged_int8.onnx",
                        53_693_315,
                        "fa3ef9902734ce5ae6f9ef2bdb2ba9a6c4b5785b09f4f420ce036573dc9d090b",
                    ),
                    (
                        "vocab.json",
                        "vocab.json",
                        1_036_584,
                        "50d6a919f0a0601d56a04eb583c780d18553aa388254ba3158eb6a00f13e2c1a",
                    ),
                    (
                        "added_tokens.json",
                        "added_tokens.json",
                        34_604,
                        "9715fd2243b6f06a5858b5e32950d2853f73dd5bc201aafcf76f5082a2d8acd1",
                    ),
                    (
                        "config.json",
                        "config.json",
                        2_243,
                        "f4d0608f7d918166da7edb3e188de5ef1bfe70d9802e785d271fd88111e9cf4b",
                    ),
                ],
            ),
            SpeechModel::Small => whisper(
                WHISPER_SMALL_REVISION,
                [
                    (
                        "encoder_model_int8.onnx",
                        "onnx/encoder_model_int8.onnx",
                        92_326_127,
                        "2601c9eb2d345c5916d4576d36f663a7c96589740fb2273828c48c3fc2c7db75",
                    ),
                    (
                        "decoder_model_merged_int8.onnx",
                        "onnx/decoder_model_merged_int8.onnx",
                        156_750_845,
                        "ec07c3cbb64172c39791e26ee870a65ac22b458c36722bfe2776b3dbf741e0c9",
                    ),
                    (
                        "vocab.json",
                        "vocab.json",
                        1_036_584,
                        "50d6a919f0a0601d56a04eb583c780d18553aa388254ba3158eb6a00f13e2c1a",
                    ),
                    (
                        "added_tokens.json",
                        "added_tokens.json",
                        34_604,
                        "9715fd2243b6f06a5858b5e32950d2853f73dd5bc201aafcf76f5082a2d8acd1",
                    ),
                    (
                        "config.json",
                        "config.json",
                        2_227,
                        "457854d452f17661e197d74aee12b8e74fb75ba30ebfaa7426d0d61ea1e08a18",
                    ),
                ],
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
            Engine::Whisper => match self {
                SpeechModel::Small => "onnx-community/whisper-small",
                _ => "onnx-community/whisper-base",
            },
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

    /// The model's directory, which the transcriber loads.
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
        self.start_download_inner(model, on_status, false)
    }

    /// Like [`Self::start_download`], but continues from the `<name>.part`
    /// files an interrupted download left on disk (HTTP `Range`; a server
    /// that ignores it makes the file restart from zero). The hash is
    /// still checked over the whole file.
    pub fn resume_download(
        &self,
        model: SpeechModel,
        on_status: StatusCallback,
    ) -> Result<(), ModelManagerError> {
        self.start_download_inner(model, on_status, true)
    }

    fn start_download_inner(
        &self,
        model: SpeechModel,
        on_status: StatusCallback,
        resume: bool,
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
            let result = download(&dir, model, &source, &slots, rx, &on_status, resume).await;
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

impl ModelManager {
    /// Removes only the leftover `<name>.part` files of the model; an
    /// installed model is never touched (issue #1332). Refused while a
    /// download runs.
    pub fn delete_partial(&self, model: SpeechModel) -> Result<(), ModelManagerError> {
        let slots = lock_tolerating_poison(&self.slots);
        if matches!(slots.get(&model), Some(Slot::Downloading { .. })) {
            return Err(ModelManagerError::Downloading);
        }
        for f in &self.source_of(model).files {
            match std::fs::remove_file(part_path(&self.dir, model, f)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(ModelManagerError::Io),
            }
        }
        // Only succeeds on an empty directory (nothing installed there).
        let _ = std::fs::remove_dir(files_dir(&self.dir, model));
        Ok(())
    }
}

/// Directory the files of a model live in: the model's own directory.
fn files_dir(dir: &Path, model: SpeechModel) -> PathBuf {
    dir.join(model.file_name())
}

/// Deletes the old whisper.cpp model files (`ggml-*.bin`, and their `.part`
/// leftovers) from the models directory. Local file operation only, never
/// outside `dir`, never touches the network; a missing file is fine. Called
/// once at start-up so the models report `not_downloaded` and the next use
/// asks for consent to download the new format.
pub fn remove_legacy_files(dir: &Path) {
    for name in LEGACY_FILES {
        for file in [name.to_owned(), format!("{name}.part")] {
            match std::fs::remove_file(dir.join(&file)) {
                Ok(()) => tracing::info!(file, "removed legacy speech model file"),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    tracing::warn!(file, error = %e, "could not remove legacy speech model file")
                }
            }
        }
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
    // Something foreign in there: leave the directory (remove_dir only
    // succeeds on an empty one).
    let _ = std::fs::remove_dir(files_dir(dir, model));
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
    resume: bool,
) -> Result<(), Option<ModelErrorKey>> {
    let result = download_all(dir, model, source, slots, &mut cancel, on_status, resume).await;
    if result.is_err() {
        for f in &source.files {
            let _ = tokio::fs::remove_file(part_path(dir, model, f)).await;
        }
        // Only removes the directory when it is empty.
        let _ = tokio::fs::remove_dir(files_dir(dir, model)).await;
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
    resume: bool,
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
            resume,
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
    resume: bool,
) -> Result<(), Option<ModelErrorKey>> {
    let io = |_| Some(ModelErrorKey::Io);
    // Bytes of an earlier attempt that are kept (only when resuming and the
    // leftover is not larger than the file).
    let existing = match tokio::fs::metadata(part).await {
        Ok(m) if resume && m.len() <= file.size => m.len(),
        _ => 0,
    };
    if existing == 0 {
        let _ = tokio::fs::remove_file(part).await;
    }
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    let mut out;
    let mut stream = None;
    if existing > 0 && existing == file.size {
        // Everything is on disk already: only the hash check is left.
        done = hash_existing(part, &mut hasher, existing, cancel).await?;
        out = tokio::fs::OpenOptions::new()
            .append(true)
            .open(part)
            .await
            .map_err(io)?;
    } else {
        let mut request = client.get(&file.url);
        if existing > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
        }
        let response = tokio::select! {
            _ = cancel.changed() => return Err(None),
            r = request.send() => r,
        }
        .map_err(|_| Some(ModelErrorKey::Network))?;
        let status = response.status();
        if existing > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT {
            done = hash_existing(part, &mut hasher, existing, cancel).await?;
            out = tokio::fs::OpenOptions::new()
                .append(true)
                .open(part)
                .await
                .map_err(io)?;
        } else if status.is_success() {
            // A plain 200 means the server ignored `Range` (or this is a
            // fresh download): the body is the whole file, start from zero.
            out = tokio::fs::File::create(part).await.map_err(io)?;
        } else {
            return Err(Some(ModelErrorKey::HttpStatus));
        }
        stream = Some(response.bytes_stream());
    }
    if done > 0 {
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
    while let Some(stream) = stream.as_mut() {
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

/// Feeds the first `len` bytes of `part` into the hasher (so the hash covers
/// the whole file after a resume); returns the number of bytes read.
async fn hash_existing(
    part: &Path,
    hasher: &mut Sha256,
    len: u64,
    cancel: &mut watch::Receiver<bool>,
) -> Result<u64, Option<ModelErrorKey>> {
    let io = |_| Some(ModelErrorKey::Io);
    let mut f = tokio::fs::File::open(part).await.map_err(io)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut read: u64 = 0;
    while read < len {
        if *cancel.borrow() {
            return Err(None);
        }
        let want = (len - read).min(buf.len() as u64) as usize;
        let n = f.read(&mut buf[..want]).await.map_err(io)?;
        if n == 0 {
            // The file shrank under us; the hash check cannot succeed.
            return Err(Some(ModelErrorKey::HashMismatch));
        }
        hasher.update(&buf[..n]);
        read += n as u64;
    }
    Ok(read)
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
