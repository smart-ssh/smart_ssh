//! Issue #90: Sprache des Sitzungs-System-Prompts aus der UI-Sprache.
//!
//! Liest nur die Eingaben — die gespeicherte Wahl (`language` in
//! `settings.json`, derselbe Store wie `frontend/src/i18n.ts`) und die
//! System-Locale über `tauri-plugin-os` (dieselbe Quelle wie
//! `detectedSystemLanguage` im Frontend). Die Regel selbst ist rein und
//! liegt in `app_logic::system_prompt::prompt_language`.
//!
//! Gelesen wird bei jedem Aufbau des System-Kontexts, nicht einmal pro
//! Programmlauf: Ein Sprachwechsel in den Einstellungen gilt damit ab dem
//! nächsten Aufbau (Verbindung oder nächste Nutzer-Nachricht, vor der der
//! Kontext neu gebaut wird), ohne Neustart.

use app_logic::system_prompt::{prompt_language, PromptLanguage};
use tauri_plugin_store::StoreExt;

/// Derselbe Ablageort wie die UI-Sprache (Spec 0024, Abschnitt 4).
use crate::settings_store::SETTINGS_STORE_FILE;
const LANGUAGE_KEY: &str = "language";

/// Ein nicht öffenbarer Store zählt wie „keine gespeicherte Wahl" — genau
/// wie im Frontend (`savedLanguage` gibt dann `null` zurück). Beide
/// Sprachfassungen tragen dieselben Sicherheitshinweise, ein Rückfall auf
/// die Locale-Regel schwächt also nichts ab.
pub(crate) fn session_prompt_language<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
) -> PromptLanguage {
    let stored = match app.store(SETTINGS_STORE_FILE) {
        Ok(store) => store.get(LANGUAGE_KEY),
        Err(err) => {
            tracing::warn!(
                error = %err,
                "settings.json nicht lesbar — System-Prompt-Sprache folgt der System-Locale",
            );
            None
        }
    };
    prompt_language(stored.as_ref(), tauri_plugin_os::locale().as_deref())
}

#[cfg(test)]
pub(crate) mod test_support {
    use tauri::test::MockRuntime;
    use tauri::AppHandle;
    use tauri_plugin_store::StoreExt;

    /// Setzt die UI-Sprache im Store dieser App-Instanz, ohne zu speichern
    /// — nur für die Dauer dieser Instanz, kein Übersprechen in andere
    /// Tests.
    pub(crate) fn set_ui_language(handle: &AppHandle<MockRuntime>, language: &str) {
        handle
            .store(super::SETTINGS_STORE_FILE)
            .expect("settings store must open in the mock app")
            .set(super::LANGUAGE_KEY, language);
    }

    /// Entfernt die UI-Sprache wieder aus dem Store dieser App-Instanz.
    pub(crate) fn clear_ui_language(handle: &AppHandle<MockRuntime>) {
        handle
            .store(super::SETTINGS_STORE_FILE)
            .expect("settings store must open in the mock app")
            .delete(super::LANGUAGE_KEY);
    }
}
