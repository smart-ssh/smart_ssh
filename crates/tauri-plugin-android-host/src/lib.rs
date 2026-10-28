//! Android host plugins that belong to the core's Rust code (ADR 0153).
//!
//! The crate ships an Android library module (`android/`) with the foreground
//! session service (`SessionServicePlugin`, `SessionService`) and the
//! microphone permission plugin (`MicrophonePlugin`), including their manifest
//! entries and strings. An app crate that embeds `app-shell` lists this crate
//! as a **direct** dependency; the Tauri CLI then adds the library to the app's
//! Gradle build and Android merges its manifest.
//!
//! Neither plugin has commands for the webview; Rust calls them through the
//! [`PluginHandle`] kept in managed state. On every non-Android target the
//! plugins are registered but empty, so both registration chains stay identical.

use tauri::plugin::{Builder, TauriPlugin};
use tauri::Runtime;

#[cfg(target_os = "android")]
use tauri::{plugin::PluginHandle, Manager};

/// Kotlin package of the library module (its Gradle `namespace`).
#[cfg(target_os = "android")]
const ANDROID_PACKAGE: &str = "dev.smartssh.androidhost";

/// Handle to the Kotlin `SessionServicePlugin` (Android only).
#[cfg(target_os = "android")]
pub struct SessionServiceHandle<R: Runtime>(pub PluginHandle<R>);

/// Handle to the Kotlin `MicrophonePlugin` (Android only).
#[cfg(target_os = "android")]
pub struct MicrophoneHandle<R: Runtime>(pub PluginHandle<R>);

/// The foreground session service plugin. On Android it registers the Kotlin
/// plugin and manages a [`SessionServiceHandle`]; elsewhere it does nothing.
pub fn session_service<R: Runtime>() -> TauriPlugin<R> {
    let builder = Builder::new("session-service");
    #[cfg(target_os = "android")]
    let builder = builder.setup(|app, api| {
        let handle = api.register_android_plugin(ANDROID_PACKAGE, "SessionServicePlugin")?;
        app.manage(SessionServiceHandle(handle));
        Ok(())
    });
    builder.build()
}

/// The microphone permission plugin. On Android it registers the Kotlin plugin
/// and manages a [`MicrophoneHandle`]; elsewhere it does nothing.
pub fn microphone<R: Runtime>() -> TauriPlugin<R> {
    let builder = Builder::new("microphone");
    #[cfg(target_os = "android")]
    let builder = builder.setup(|app, api| {
        let handle = api.register_android_plugin(ANDROID_PACKAGE, "MicrophonePlugin")?;
        app.manage(MicrophoneHandle(handle));
        Ok(())
    });
    builder.build()
}
