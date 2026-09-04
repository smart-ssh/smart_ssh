//! Erststart-Hinweis (Spec 0031): Verantwortungs-/Verschlüsselungs-Hinweis,
//! der einmalig vor der ersten Server-Verbindung bestätigt werden muss.

use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

/// Spec 0024-Muster (s. `crate::risk_second_opinion`-Moduldoc): derselbe
/// `tauri-plugin-store`-Ablageort wie die übrigen reinen UI-/App-
/// Einstellungen, kein SQLite nötig für ein einzelnes Bool.
const SETTINGS_STORE_FILE: &str = "settings.json";
const ACKNOWLEDGED_KEY: &str = "first_run_notice_acknowledged";

/// Spec 0031, Abschnitt 4, letzter Punkt: `connect_session` prüft dies
/// **serverseitig** zusätzlich zur Frontend-Sperre in `ServerList.tsx` —
/// eine reine Frontend-Sperre wäre umgehbar (z. B. ein direkter
/// `invoke("connect", ...)`-Aufruf, der die UI-Sperre gar nicht durchläuft).
/// `false` sowohl bei explizit gespeichertem `false` als auch bei fehlendem
/// Store/Schlüssel (Erststart) — Fail-safe: im Zweifel gilt der Hinweis als
/// nicht bestätigt, nie automatisch als bestätigt.
pub fn is_acknowledged<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Ok(store) = app.store(SETTINGS_STORE_FILE) else {
        return false;
    };
    store
        .get(ACKNOWLEDGED_KEY)
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Test-Infrastruktur, auch von `crate::commands`s
/// `connect_session_gate_tests` genutzt (daher `pub(crate)`, nicht nur
/// `#[cfg(test)]` privat in diesem Modul).
///
/// Wichtige Einschränkung von `tauri::test::mock_context`: Der
/// `identifier` im gemockten `Config` ist ein fixer Default-Leerstring,
/// wodurch `BaseDirectory::AppData` (die Basis, gegen die
/// `tauri_plugin_store` seinen relativen "settings.json"-Pfad aus
/// Produktivcode auflöst) für **alle** Tests, die `mock_context` nutzen,
/// auf denselben echten Ordner zeigt (`~/Library/Application Support/`
/// bzw. Plattform-Äquivalent) — nicht in ein isoliertes Test-Verzeichnis.
/// Ein `tempfile::tempdir()` hilft hier nicht: `StoreBuilder` löst seinen
/// Pfad *immer* über `BaseDirectory::AppData` auf, auch bei einem bereits
/// absoluten Pfad. Deshalb hier bewusst: (1) ein globaler Mutex, der alle
/// Store-anfassenden Tests serialisiert (verhindert parallele
/// Lese-/Schreib-Races auf derselben echten Datei), (2) jeder Test setzt
/// seinen eigenen Ausgangszustand explizit (nie stillschweigende Annahme
/// eines "sauberen" Zustands), (3) `reset()` räumt am Ende wieder auf.
#[cfg(test)]
pub(crate) mod test_support {
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
    use tauri::AppHandle;
    use tauri_plugin_store::StoreExt;
    use tokio::sync::{Mutex, MutexGuard};

    // `tokio::sync::Mutex` statt `std::sync::Mutex` (Spec 0032, Abschnitt
    // 8-Test: `crate::commands::local_server_tests` hält diese Sperre über
    // einen `.await`-Punkt hinweg — ein `std::sync::MutexGuard` dort wäre
    // ein Clippy-`await_holding_lock`-Fehler, der async-fähige Typ ist hier
    // die eigentliche Lösung, kein bloßes Unterdrücken der Warnung).
    static STORE_TEST_LOCK: Mutex<()> = Mutex::const_new(());

    /// Muss von jedem synchronen Test gehalten werden, der `test_app()`/den
    /// `"settings.json"`-Store anfasst — s. Moduldoc. `blocking_lock()`
    /// funktioniert hier (kein Tokio-Executor im Kontext eines einfachen
    /// `#[test]`s); für `#[tokio::test]`s, die die Sperre über einen
    /// `await`-Punkt halten müssen, stattdessen [`lock_async`] verwenden.
    pub(crate) fn lock() -> MutexGuard<'static, ()> {
        STORE_TEST_LOCK.blocking_lock()
    }

    /// Async-Variante von [`lock`] für `#[tokio::test]`s, die die Sperre
    /// über einen `await`-Punkt hinweg halten (z. B. weil sie einen
    /// `#[tauri::command]`-Kern wie `list_servers_impl` aufrufen).
    pub(crate) async fn lock_async() -> MutexGuard<'static, ()> {
        STORE_TEST_LOCK.lock().await
    }

    pub(crate) fn test_app() -> tauri::App<MockRuntime> {
        mock_builder()
            .plugin(tauri_plugin_store::Builder::new().build())
            .build(mock_context(noop_assets()))
            .expect("mock app konnte nicht gebaut werden")
    }

    /// Löscht den Bestätigungs-Schlüssel wieder — Aufrufer muss bereits
    /// `lock()` halten.
    pub(crate) fn reset(handle: &AppHandle<MockRuntime>) {
        if let Ok(store) = handle.store(super::SETTINGS_STORE_FILE) {
            store.delete(super::ACKNOWLEDGED_KEY);
            let _ = store.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{lock, reset, test_app};
    use super::*;

    #[test]
    fn test_is_acknowledged_defaults_to_false_when_never_set() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle().clone();
        reset(&handle);

        assert!(!is_acknowledged(&handle));
    }

    #[test]
    fn test_is_acknowledged_true_after_being_set() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle().clone();
        reset(&handle);
        let store = handle
            .store(SETTINGS_STORE_FILE)
            .expect("Store sollte sich öffnen lassen");
        store.set(ACKNOWLEDGED_KEY, serde_json::json!(true));

        assert!(is_acknowledged(&handle));
        reset(&handle);
    }

    #[test]
    fn test_is_acknowledged_false_when_explicitly_false() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle().clone();
        reset(&handle);
        let store = handle
            .store(SETTINGS_STORE_FILE)
            .expect("Store sollte sich öffnen lassen");
        store.set(ACKNOWLEDGED_KEY, serde_json::json!(false));

        assert!(!is_acknowledged(&handle));
    }
}
