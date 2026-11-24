//! Tests of the first-run-notice gate in `connect_session`.

use super::*;
use crate::first_run_notice::test_support::{lock, reset, test_app};
use tauri_plugin_store::StoreExt;

/// Spec 0031, Abschnitt 4: "`connect()` schlägt fehl/wird blockiert,
/// solange `first_run_notice_acknowledged` `false` ist" — geprüft am
/// exakten Türsteher-Schritt, den `connect_session` als Allererstes
/// aufruft, bevor irgendein anderer Teil der Verbindungslogik läuft.
/// `_guard`/`reset(...)`: s. `first_run_notice::test_support`-Moduldoc
/// — dieser Test teilt sich denselben echten Store mit
/// `first_run_notice::tests` und muss daher denselben Mutex halten und
/// seinen eigenen Ausgangszustand explizit herstellen.
#[test]
fn test_connect_session_gate_blocks_when_not_acknowledged() {
    let _guard = lock();
    let app = test_app();
    let handle = app.handle().clone();
    reset(&handle);

    let err = ensure_first_run_notice_acknowledged(&handle)
        .expect_err("Erststart-Hinweis wurde nie bestätigt, muss fehlschlagen");
    assert_eq!(err.code, Some("FIRST_RUN_NOTICE_NOT_ACKNOWLEDGED"));
}

#[test]
fn test_connect_session_gate_passes_once_acknowledged() {
    let _guard = lock();
    let app = test_app();
    let handle = app.handle().clone();
    reset(&handle);
    let store = handle
        .store("settings.json")
        .expect("Store sollte sich öffnen lassen");
    store.set("first_run_notice_acknowledged", serde_json::json!(true));

    assert!(ensure_first_run_notice_acknowledged(&handle).is_ok());
    reset(&handle);
}

/// Spec 0031, Abschnitt 4: "Bestätigung setzt die Einstellung korrekt
/// und dauerhaft (übersteht einen simulierten Neustart)" — eine zweite,
/// unabhängig aufgebaute `App`-Instanz (= simulierter Neustart) muss
/// den zuvor per `.save()` auf die Festplatte geschriebenen Wert
/// wiederfinden, nicht nur innerhalb derselben `App`-Instanz.
#[test]
fn test_acknowledgement_persists_across_a_simulated_restart() {
    let _guard = lock();
    {
        let first_run_app = test_app();
        let handle = first_run_app.handle().clone();
        reset(&handle);
        let store = handle
            .store("settings.json")
            .expect("Store sollte sich öffnen lassen");
        store.set("first_run_notice_acknowledged", serde_json::json!(true));
        store.save().expect("Store sollte sich speichern lassen");
    }

    // Neue, komplett unabhängige `App`-Instanz mit eigenem
    // Store-Cache — simuliert einen App-Neustart, bei dem nichts mehr
    // im Speicher steht außer dem, was tatsächlich auf der Platte
    // gelandet ist (`store.save()` oben).
    let restarted_app = test_app();
    let restarted_handle = restarted_app.handle().clone();
    assert!(ensure_first_run_notice_acknowledged(&restarted_handle).is_ok());
    reset(&restarted_handle);
}
