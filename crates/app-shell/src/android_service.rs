//! Android: keeps open SSH sessions alive in the background (Spec 0114 §4,
//! ADR 0149). Bridges `app_logic::background_keepalive` to the Kotlin plugin
//! `SessionServicePlugin` (foreground service, notification, wake lock).

use std::sync::mpsc;
use std::sync::Arc;

use app_logic::background_keepalive::{self, ForegroundService, KeepAliveController};
use app_logic::state::AppState;
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::PluginHandle;
use tauri::{AppHandle, Manager, Wry};

/// Commands for the Kotlin plugin, executed in order on one thread so that
/// the (blocking) plugin call never stalls an async task.
enum Call {
    Start(usize),
    Update(usize),
    Stop,
    WakeLock(bool),
}

struct AndroidForeground {
    tx: mpsc::Sender<Call>,
}

impl ForegroundService for AndroidForeground {
    fn start(&self, sessions: usize) -> Result<(), String> {
        self.send(Call::Start(sessions))
    }
    fn update(&self, sessions: usize) -> Result<(), String> {
        self.send(Call::Update(sessions))
    }
    fn stop(&self) -> Result<(), String> {
        self.send(Call::Stop)
    }
    fn set_wake_lock(&self, held: bool) -> Result<(), String> {
        self.send(Call::WakeLock(held))
    }
}

impl AndroidForeground {
    fn send(&self, call: Call) -> Result<(), String> {
        self.tx.send(call).map_err(|e| e.to_string())
    }
}

fn disconnect_all(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let ids: Vec<_> = app
            .state::<AppState>()
            .sessions
            .snapshot()
            .into_iter()
            .map(|entry| entry.session_id)
            .collect();
        for id in ids {
            let result = crate::commands::disconnect(app.clone(), id).await;
            if let Err(error) = result {
                tracing::warn!(session_id = %id, ?error, "disconnect all: session not closed");
            }
        }
    });
}

fn run_calls(handle: PluginHandle<Wry>, rx: mpsc::Receiver<Call>, app: AppHandle) {
    while let Ok(call) = rx.recv() {
        let (name, payload) = match call {
            Call::Start(sessions) => {
                let app = app.clone();
                let channel = Channel::<Value>::new(move |body| {
                    if let InvokeResponseBody::Json(json) = body {
                        if json.contains("disconnectAll") {
                            disconnect_all(&app);
                        }
                    }
                    Ok(())
                });
                (
                    "startService",
                    json!({ "sessions": sessions, "onEvent": channel }),
                )
            }
            Call::Update(sessions) => ("updateService", json!({ "sessions": sessions })),
            Call::Stop => ("stopService", json!({})),
            Call::WakeLock(held) => ("setWakeLock", json!({ "held": held })),
        };
        if let Err(error) = handle.run_mobile_plugin::<()>(name, payload) {
            tracing::warn!(%error, command = name, "session service plugin call failed");
        }
    }
}

/// Registers the plugin and wires the session count to it. Idempotent.
pub(crate) fn start(app: &AppHandle) {
    if app.try_state::<Installed>().is_some() {
        return;
    }
    app.manage(Installed);
    let (tx, rx) = mpsc::channel();
    let (handle_tx, handle_rx) = mpsc::channel();
    let plugin = tauri::plugin::Builder::<Wry>::new("session-service")
        .setup(move |_app, api| {
            let handle =
                api.register_android_plugin("com.smartssh.desktop", "SessionServicePlugin")?;
            let _ = handle_tx.send(handle);
            Ok(())
        })
        .build();
    if let Err(error) = app.plugin(plugin) {
        tracing::error!(%error, "session service plugin could not be registered");
        return;
    }
    let Ok(handle) = handle_rx.recv() else {
        tracing::error!("session service plugin handle missing");
        return;
    };
    let thread_app = app.clone();
    std::thread::spawn(move || run_calls(handle, rx, thread_app));

    let controller = KeepAliveController::new(Arc::new(AndroidForeground { tx }));
    background_keepalive::install(Arc::clone(&controller));
    let mut count = app.state::<AppState>().sessions.subscribe_count();
    tauri::async_runtime::spawn(async move {
        controller.on_session_count(*count.borrow_and_update());
        while count.changed().await.is_ok() {
            controller.on_session_count(*count.borrow_and_update());
        }
    });
}

struct Installed;
