//! Voice input, recording (Spec 0108, section 5): thin Tauri layer over the
//! `speech` crate's recorder. The microphone is only opened by
//! `start_voice_recording`, which only a user action calls.

use std::sync::{Arc, OnceLock};

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_store::StoreExt;

use app_logic::error::{CommandError, CommandResult};
use speech::{
    CpalSource, LiveConfig, ModelStatus, RecordError, Recorder, RecorderState, WhisperTranscriber,
    MAX_RECORDING,
};

use crate::settings_store::{self, SETTINGS_STORE_FILE};
use crate::speech_models;

const LANGUAGE_KEY: &str = "speechLanguage";
/// Event with `{ state: "recording" | "transcribing" | "idle" | "finished" }`.
/// `finished` follows an automatic stop at the maximum length; the text is
/// collected with `stop_voice_recording`.
pub(crate) const STATE_EVENT: &str = "speech-recording-state";
/// Event with `{ text, revision }`: the provisional text of the running
/// recording so far, while recording only (Spec 0108, 5.7). It stays on the
/// machine; the final text is still only returned by `stop_voice_recording`.
pub(crate) const PARTIAL_EVENT: &str = "speech-partial-text";

#[derive(Default)]
pub struct VoiceRecorder(OnceLock<Recorder>);

fn recorder<R: Runtime>(app: &AppHandle<R>) -> Recorder {
    let state = app.state::<VoiceRecorder>();
    state
        .0
        .get_or_init(|| {
            let emit_app = app.clone();
            let partial_app = app.clone();
            Recorder::with_live(
                Arc::new(CpalSource),
                Arc::new(WhisperTranscriber),
                MAX_RECORDING,
                Arc::new(move |s: RecorderState| {
                    let _ = emit_app.emit(STATE_EVENT, serde_json::json!({ "state": s }));
                }),
                Arc::new(move |revision: u64, text: String| {
                    let _ = partial_app.emit(
                        PARTIAL_EVENT,
                        serde_json::json!({ "text": text, "revision": revision }),
                    );
                }),
                LiveConfig::default(),
                tauri::async_runtime::handle().inner().clone(),
            )
        })
        .clone()
}

fn record_error(err: RecordError) -> CommandError {
    CommandError::with_code(format!("voice input failed: {err:?}"), err.key())
}

/// Stored language setting; `None` = auto-detect. An unusable stored value
/// falls back to auto.
pub(crate) fn read_language<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    app.store(SETTINGS_STORE_FILE)
        .ok()
        .and_then(|s| s.get(LANGUAGE_KEY))
        .and_then(|v| v.as_str().map(str::to_owned))
        .and_then(|s| speech::parse_language(&s).ok().flatten())
}

#[tauri::command]
pub fn get_speech_language(app: AppHandle) -> CommandResult<String> {
    Ok(read_language(&app).unwrap_or_else(|| "auto".to_owned()))
}

/// `auto` or an ISO language code Whisper supports.
#[tauri::command]
pub fn set_speech_language(app: AppHandle, language: String) -> CommandResult<()> {
    let parsed = speech::parse_language(&language).map_err(|_| {
        CommandError::with_code(
            format!("unknown speech language: {language}"),
            "speech_language_invalid",
        )
    })?;
    let value = parsed.unwrap_or_else(|| "auto".to_owned());
    settings_store::update(&app, |changes| {
        changes.set(LANGUAGE_KEY, serde_json::json!(value))
    })
}

/// Android: asks for the `RECORD_AUDIO` runtime permission through a small
/// native plugin (Spec 0108, 5.7). The system dialog appears on the first
/// start; a denial surfaces as `speech_permission_denied`, never a crash.
#[cfg(target_os = "android")]
mod android_permission {
    use serde::Deserialize;
    use tauri::{AppHandle, Manager, Runtime};
    use tauri_plugin_android_host::MicrophoneHandle as Handle;

    #[derive(Deserialize)]
    struct PermissionResult {
        granted: bool,
    }

    /// `Ok(true)` when the permission is granted (asking if needed).
    /// Any failure to ask counts as not granted (fail closed).
    pub fn ensure_granted<R: Runtime>(app: &AppHandle<R>) -> bool {
        let Some(handle) = app.try_state::<Handle<R>>() else {
            return false;
        };
        match handle
            .0
            .run_mobile_plugin::<PermissionResult>("requestMicrophone", ())
        {
            Ok(r) => r.granted,
            Err(e) => {
                tracing::warn!(error = %e, "microphone permission request failed");
                false
            }
        }
    }
}

/// Starts recording. Errors with `speech_no_model` before the input device
/// is touched when the selected model is not ready.
#[tauri::command(async)]
pub fn start_voice_recording(app: AppHandle) -> CommandResult<()> {
    let selected = speech_models::read_selected_model(&app);
    let model = speech_models::manager(&app).ok().and_then(|m| {
        matches!(m.status(selected), ModelStatus::Ready).then(|| m.model_path(selected))
    });
    // Ask for the microphone only once a model is ready, so the permission
    // dialog never precedes the download consent. Without a model `start`
    // fails with `speech_no_model` before the device is touched.
    #[cfg(target_os = "android")]
    if model.is_some() && !android_permission::ensure_granted(&app) {
        return Err(record_error(RecordError::PermissionDenied));
    }
    recorder(&app)
        .start(model, read_language(&app))
        .map_err(record_error)
}

/// Stops recording, transcribes locally and returns the text.
#[tauri::command]
pub async fn stop_voice_recording(app: AppHandle) -> CommandResult<String> {
    recorder(&app).stop().await.map_err(record_error)
}

/// Discards the recording; nothing is transcribed.
#[tauri::command]
pub fn cancel_voice_recording(app: AppHandle) -> CommandResult<()> {
    recorder(&app).cancel();
    Ok(())
}
