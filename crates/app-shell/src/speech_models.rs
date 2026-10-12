//! Voice input, model management (Spec 0108): thin Tauri layer over the
//! `speech` crate. Download only on an explicit command, never at start-up.

use std::sync::{Arc, OnceLock};

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_store::StoreExt;

use app_logic::error::{CommandError, CommandResult};
use speech::{ModelManager, ModelManagerError, ModelStatus, SpeechModel};

use crate::settings_store::{self, SETTINGS_STORE_FILE};

const SELECTED_MODEL_KEY: &str = "speechModel";
const MODELS_SUBDIR: &str = "speech-models";
/// Event with `{ model, state, done?, total?, key? }`, emitted on every
/// progress step and on the final state of a download.
pub(crate) const STATUS_EVENT: &str = "speech-model-status";

/// Lazily created so nothing touches the disk before the first command.
#[derive(Default)]
pub struct SpeechModels(OnceLock<Arc<ModelManager>>);

fn manager<R: Runtime>(app: &AppHandle<R>) -> CommandResult<Arc<ModelManager>> {
    let state = app.state::<SpeechModels>();
    if let Some(m) = state.0.get() {
        return Ok(Arc::clone(m));
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::from(e.to_string()))?
        .join(MODELS_SUBDIR);
    Ok(Arc::clone(
        state.0.get_or_init(|| Arc::new(ModelManager::new(dir))),
    ))
}

fn parse_model(id: &str) -> CommandResult<SpeechModel> {
    SpeechModel::from_id(id).ok_or_else(|| {
        CommandError::with_code(
            format!("unknown speech model: {id}"),
            "speech_model_unknown",
        )
    })
}

fn status_payload(model: SpeechModel, status: &ModelStatus) -> serde_json::Value {
    let mut v = serde_json::to_value(status).unwrap_or_default();
    v["model"] = model.id().into();
    v
}

fn manager_error(err: ModelManagerError) -> CommandError {
    let code = match err {
        ModelManagerError::AlreadyDownloading | ModelManagerError::Downloading => {
            "speech_model_busy"
        }
        ModelManagerError::Io => "speech_model_io",
    };
    CommandError::with_code(format!("speech model operation failed: {err:?}"), code)
}

pub(crate) fn read_selected_model<R: Runtime>(app: &AppHandle<R>) -> SpeechModel {
    app.store(SETTINGS_STORE_FILE)
        .ok()
        .and_then(|s| s.get(SELECTED_MODEL_KEY))
        .and_then(|v| v.as_str().and_then(SpeechModel::from_id))
        .unwrap_or(SpeechModel::DEFAULT)
}

pub(crate) fn write_selected_model<R: Runtime>(
    app: &AppHandle<R>,
    model: SpeechModel,
) -> CommandResult<()> {
    settings_store::update(app, |changes| {
        changes.set(SELECTED_MODEL_KEY, serde_json::json!(model.id()))
    })
}

#[tauri::command]
pub fn get_selected_speech_model(app: AppHandle) -> CommandResult<String> {
    Ok(read_selected_model(&app).id().to_owned())
}

#[tauri::command]
pub fn set_selected_speech_model(app: AppHandle, model: String) -> CommandResult<()> {
    write_selected_model(&app, parse_model(&model)?)
}

/// Status of one model (local state only, no network).
#[tauri::command]
pub fn get_speech_model_status(app: AppHandle, model: String) -> CommandResult<serde_json::Value> {
    let model = parse_model(&model)?;
    Ok(status_payload(model, &manager(&app)?.status(model)))
}

/// Starts the download. Only ever called from an explicit user action.
#[tauri::command]
pub async fn start_speech_model_download(app: AppHandle, model: String) -> CommandResult<()> {
    let model = parse_model(&model)?;
    let emit_app = app.clone();
    manager(&app)?
        .start_download(
            model,
            Arc::new(move |m, status| {
                let _ = emit_app.emit(STATUS_EVENT, status_payload(m, &status));
            }),
        )
        .map_err(manager_error)
}

#[tauri::command]
pub fn cancel_speech_model_download(app: AppHandle, model: String) -> CommandResult<()> {
    manager(&app)?.cancel_download(parse_model(&model)?);
    Ok(())
}

#[tauri::command]
pub fn delete_speech_model(app: AppHandle, model: String) -> CommandResult<()> {
    manager(&app)?
        .delete(parse_model(&model)?)
        .map_err(manager_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::first_run_notice::test_support::{lock, test_app};

    #[test]
    fn selected_model_defaults_to_base_and_round_trips() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle();
        settings_store::update(handle, |c| c.delete(SELECTED_MODEL_KEY)).unwrap();
        assert_eq!(read_selected_model(handle), SpeechModel::Base);

        write_selected_model(handle, SpeechModel::Small).unwrap();
        assert_eq!(read_selected_model(handle), SpeechModel::Small);

        write_selected_model(handle, SpeechModel::Base).unwrap();
        assert_eq!(read_selected_model(handle), SpeechModel::Base);
    }

    #[test]
    fn unknown_model_id_is_rejected() {
        assert!(parse_model("tiny").is_err());
        assert!(parse_model("base").is_ok());
    }

    #[test]
    fn status_payload_carries_model_and_state() {
        let v = status_payload(
            SpeechModel::Small,
            &ModelStatus::Downloading { done: 1, total: 2 },
        );
        assert_eq!(v["model"], "small");
        assert_eq!(v["state"], "downloading");
        assert_eq!(v["done"], 1);
        assert_eq!(
            status_payload(SpeechModel::Base, &ModelStatus::NotDownloaded)["state"],
            "not_downloaded"
        );
    }
}
