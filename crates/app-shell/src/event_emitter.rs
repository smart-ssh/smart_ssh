//! Spec 0084, §4 (Schnitt „`EventEmitter`-Impl für `AppHandle`"): Newtype um
//! `tauri::AppHandle`, damit diese Impl in `app-shell` bleiben kann, auch
//! wenn der `EventEmitter`-Trait selbst nach `app-logic` zieht.
//!
//! `impl EventEmitter for tauri::AppHandle` wäre dort verboten (Orphan-Rule:
//! weder der Trait noch `AppHandle` sind in `app-shell` definiert) — dieser
//! Newtype IST in `app-shell` definiert, das genügt der Regel. Inhaltlich
//! unverändert gegenüber der vorherigen Impl (nur verschoben, s. Spec 0084
//! A7).

use crate::events::EventEmitter;

pub struct TauriEventEmitter<R: tauri::Runtime = tauri::Wry>(pub tauri::AppHandle<R>);

impl<R: tauri::Runtime> EventEmitter for TauriEventEmitter<R> {
    fn emit_event(&self, event: &str, payload: serde_json::Value) {
        // s. vorherige `impl EventEmitter for tauri::AppHandle` in
        // `events.rs` — dieselbe Begründung (kein harter Fehler bei
        // herunterfahrender App), nur der Ort hat sich geändert.
        if let Err(err) = tauri::Emitter::emit(&self.0, event, payload) {
            eprintln!("Event '{event}' konnte nicht gesendet werden: {err}");
        }
    }
}
