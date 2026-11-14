//! Voice input, model management (Spec 0108): thin Tauri layer over the
//! `speech` crate. Download only on an explicit command, never at start-up.

use std::sync::{Arc, Mutex, OnceLock};

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_store::StoreExt;

use app_logic::error::{CommandError, CommandResult};
use speech::{ModelManager, ModelManagerError, ModelStatus, SpeechModel};

use crate::activities::{ActivityHandle, ActivityRegistry, ActivitySpec, ResumeSpec};
use crate::settings_store;
use ssh_manager_core::sync::lock_tolerating_poison;

const SELECTED_MODEL_KEY: &str = "speechModel";
/// Event with `{ model, state, done?, total?, key? }`, emitted on every
/// progress step and on the final state of a download.
pub(crate) const STATUS_EVENT: &str = app_logic::event_names::SPEECH_MODEL_STATUS;

/// Lazily created so nothing touches the disk before the first command.
#[derive(Default)]
pub struct SpeechModels(OnceLock<Arc<ModelManager>>);

/// Where the models live: inside the data directory with an override
/// (issue #1313), otherwise below the Tauri app data directory as before.
pub(crate) fn models_dir<R: Runtime>(app: &AppHandle<R>) -> CommandResult<std::path::PathBuf> {
    // With an override the default app data dir is never consulted, so its
    // lookup failing cannot affect an override instance.
    if persistence_sqlite::data_dir_override().is_some() {
        return Ok(app_logic::data_paths::speech_models_dir(
            std::path::Path::new(""),
        ));
    }
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::with_code(e.to_string(), "SPEECH_MODELS_DIR_UNAVAILABLE"))?;
    Ok(app_logic::data_paths::speech_models_dir(&app_data))
}

pub(crate) fn manager<R: Runtime>(app: &AppHandle<R>) -> CommandResult<Arc<ModelManager>> {
    let state = app.state::<SpeechModels>();
    if let Some(m) = state.0.get() {
        return Ok(Arc::clone(m));
    }
    let dir = models_dir(app)?;
    Ok(Arc::clone(
        state.0.get_or_init(|| Arc::new(ModelManager::new(dir))),
    ))
}

/// Start-up step (Spec 0108): deletes the old whisper.cpp model files from the
/// models directory. A local file operation only; no network, no model load.
pub(crate) fn remove_legacy_files_at_start<R: Runtime>(app: &AppHandle<R>) {
    match models_dir(app) {
        Ok(dir) => speech::remove_legacy_files(&dir),
        Err(e) => {
            tracing::warn!(error = %e.message, "speech models directory unavailable at start")
        }
    }
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
    app.store(crate::settings_store::settings_store_file())
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

/// The models offered on this platform (id, engine, total size, source),
/// so the UI does not hard-code them. Static data, no network.
#[tauri::command]
pub fn list_speech_models() -> Vec<speech::ModelInfo> {
    SpeechModel::available_info()
}

/// Status of one model (local state only, no network).
#[tauri::command]
pub fn get_speech_model_status(app: AppHandle, model: String) -> CommandResult<serde_json::Value> {
    let model = parse_model(&model)?;
    Ok(status_payload(model, &manager(&app)?.status(model)))
}

/// Activity kind of a model download (Spec 0116); the key is the model id.
pub(crate) const ACTIVITY_KIND: &str = "speech-model";

fn model_display_name(model: SpeechModel) -> &'static str {
    match model {
        SpeechModel::Base => "Base",
        SpeechModel::Small => "Small",
        SpeechModel::ParakeetV3 => "Parakeet v3",
    }
}

/// Drives the activity of one download from the speech crate's status
/// callback (issue #1326). The handle is taken on the final status.
struct DownloadActivity {
    registry: Arc<ActivityRegistry>,
    handle: Mutex<Option<ActivityHandle>>,
}

impl DownloadActivity {
    fn on_status(&self, status: &ModelStatus) {
        let mut slot = lock_tolerating_poison(&self.handle);
        match status {
            ModelStatus::Downloading { done, total } => {
                if let Some(h) = slot.as_ref() {
                    h.progress(*done, Some(*total));
                    // All bytes are in; the hash check of the last file follows.
                    if *total > 0 && done >= total {
                        h.status(Some("activities.verifying"));
                    }
                }
            }
            ModelStatus::Ready => {
                if let Some(h) = slot.take() {
                    h.succeed();
                }
            }
            ModelStatus::Error { key } => {
                if let Some(h) = slot.take() {
                    h.fail(key, "speech model download failed");
                }
            }
            ModelStatus::NotDownloaded => {
                if let Some(h) = slot.take() {
                    // Cancelled from the settings: end the row as cancelled.
                    // Already cancelled from the activity center: the call is
                    // refused (the row is no longer running), which is fine.
                    let _ = self.registry.cancel(h.id());
                }
            }
        }
    }
}

/// Starts the download and its activity. Only ever called from an explicit
/// user action.
fn start_download_with_activity<R: Runtime>(
    app: &AppHandle<R>,
    model: SpeechModel,
    resume: bool,
) -> CommandResult<()> {
    let manager = manager(app)?;
    if matches!(manager.status(model), ModelStatus::Downloading { .. }) {
        return Err(manager_error(ModelManagerError::AlreadyDownloading));
    }
    let registry = crate::activities::activities(app);
    let handle = registry.start(ActivitySpec {
        kind: ACTIVITY_KIND.into(),
        key: model.id().into(),
        title_key: "activities.speechModelTitle".into(),
        title_params: [("name".to_owned(), model_display_name(model).to_owned())].into(),
        cancellable: true,
        total: None,
        // A quit keeps the `.part` files; the next start offers to resume.
        resume: Some(ResumeSpec {
            kind: ACTIVITY_KIND.into(),
            key: model.id().into(),
        }),
    });
    let id = handle.id().to_owned();
    {
        let manager = Arc::clone(&manager);
        handle.on_cancel(move || {
            manager.cancel_download(model);
        });
    }
    let activity = Arc::new(DownloadActivity {
        registry: Arc::clone(&registry),
        handle: Mutex::new(Some(handle)),
    });
    let emit_app = app.clone();
    let cb_activity = Arc::clone(&activity);
    let on_status: speech::StatusCallback = Arc::new(move |m, status| {
        cb_activity.on_status(&status);
        let _ = emit_app.emit(STATUS_EVENT, status_payload(m, &status));
    });
    let result = if resume {
        manager.resume_download(model, on_status)
    } else {
        manager.start_download(model, on_status)
    };
    if let Err(err) = result {
        // No download started (e.g. one already runs): remove the new row so
        // the running download's row stays the only one.
        if let Some(h) = lock_tolerating_poison(&activity.handle).take() {
            h.succeed();
        }
        let _ = registry.dismiss(&id);
        return Err(manager_error(err));
    }
    Ok(())
}

/// Registers the resume handler of interrupted model downloads (issue
/// #1327): "Resume" continues from the `.part` files, "Discard" deletes them.
pub(crate) fn register_resume_handler<R: Runtime>(app: &AppHandle<R>) {
    let resume_app = app.clone();
    let discard_app = app.clone();
    crate::activities::register_resume_handler(
        app,
        ACTIVITY_KIND,
        Arc::new(move |entry| {
            start_download_with_activity(&resume_app, parse_model(&entry.key)?, true)
        }),
        Some(Arc::new(move |entry| {
            let Ok(model) = parse_model(&entry.key) else {
                return;
            };
            match manager(&discard_app).map(|m| m.delete(model)) {
                Ok(Ok(())) => {}
                Ok(Err(err)) => tracing::warn!(?err, "interrupted download files not deleted"),
                Err(err) => tracing::warn!(code = ?err.code, "speech models unavailable"),
            }
        })),
    );
}

#[tauri::command]
pub async fn start_speech_model_download(app: AppHandle, model: String) -> CommandResult<()> {
    start_download_with_activity(&app, parse_model(&model)?, false)
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

    /// Issue #887: speech-model failures reach the frontend with a code.
    #[test]
    fn test_every_model_manager_error_carries_a_code() {
        for err in [
            ModelManagerError::AlreadyDownloading,
            ModelManagerError::Downloading,
            ModelManagerError::Io,
        ] {
            assert!(manager_error(err).code.is_some());
        }
    }
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
    fn parakeet_is_listed_and_selectable_on_every_platform() {
        let ids: Vec<&str> = list_speech_models().iter().map(|m| m.id).collect();
        assert_eq!(ids, ["base", "small", "parakeet_v3"]);
        assert_eq!(parse_model("parakeet_v3").unwrap(), SpeechModel::ParakeetV3);
    }

    /// A stored selection this platform does not offer falls back to the default.
    #[test]
    fn stored_unavailable_selection_falls_back_to_default() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle();
        settings_store::update(handle, |c| {
            c.set(SELECTED_MODEL_KEY, serde_json::json!("nonexistent"))
        })
        .unwrap();
        assert_eq!(read_selected_model(handle), SpeechModel::DEFAULT);
        settings_store::update(handle, |c| {
            c.set(SELECTED_MODEL_KEY, serde_json::json!("parakeet_v3"))
        })
        .unwrap();
        assert_eq!(read_selected_model(handle), SpeechModel::ParakeetV3);
        settings_store::update(handle, |c| c.delete(SELECTED_MODEL_KEY)).unwrap();
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

    fn activity_fixture(model: SpeechModel) -> (DownloadActivity, Arc<ActivityRegistry>) {
        let registry = ActivityRegistry::new(Arc::new(app_logic::events::TestEmitter::default()));
        let handle = registry.start(ActivitySpec {
            kind: ACTIVITY_KIND.into(),
            key: model.id().into(),
            title_key: "activities.speechModelTitle".into(),
            cancellable: true,
            ..Default::default()
        });
        let activity = DownloadActivity {
            registry: Arc::clone(&registry),
            handle: Mutex::new(Some(handle)),
        };
        (activity, registry)
    }

    /// Issue #1326: progress, "Verifying" at full size, success.
    #[test]
    fn activity_follows_progress_verifying_and_success() {
        let (a, reg) = activity_fixture(SpeechModel::Small);
        a.on_status(&ModelStatus::Downloading {
            done: 10,
            total: 100,
        });
        let row = reg.list().remove(0);
        assert_eq!((row.done, row.total), (10, Some(100)));
        assert_eq!(row.status_key, None);
        a.on_status(&ModelStatus::Downloading {
            done: 100,
            total: 100,
        });
        assert_eq!(
            reg.list()[0].status_key.as_deref(),
            Some("activities.verifying")
        );
        a.on_status(&ModelStatus::Ready);
        assert_eq!(
            reg.list()[0].state,
            app_logic::activities::ActivityState::Succeeded
        );
    }

    /// A hash mismatch / network error ends the activity as failed with the code.
    #[test]
    fn activity_fails_with_the_error_code() {
        let (a, reg) = activity_fixture(SpeechModel::Base);
        a.on_status(&ModelStatus::Error {
            key: "speech_model_hash_mismatch",
        });
        let row = reg.list().remove(0);
        assert_eq!(row.state, app_logic::activities::ActivityState::Failed);
        assert_eq!(
            row.error_code.as_deref(),
            Some("speech_model_hash_mismatch")
        );
    }

    /// Cancel from the settings (final `NotDownloaded`) ends the row as cancelled.
    #[test]
    fn activity_is_cancelled_when_the_download_reports_not_downloaded() {
        let (a, reg) = activity_fixture(SpeechModel::ParakeetV3);
        a.on_status(&ModelStatus::NotDownloaded);
        assert_eq!(
            reg.list()[0].state,
            app_logic::activities::ActivityState::Cancelled
        );
    }

    /// Cancel from the activity center stops the download through the manager
    /// and the later `NotDownloaded` does not disturb the cancelled row.
    #[test]
    fn activity_center_cancel_then_not_downloaded_is_stable() {
        let (a, reg) = activity_fixture(SpeechModel::Base);
        let id = reg.list()[0].id.clone();
        reg.cancel(&id).unwrap();
        a.on_status(&ModelStatus::NotDownloaded);
        assert_eq!(
            reg.list()[0].state,
            app_logic::activities::ActivityState::Cancelled
        );
    }
}
