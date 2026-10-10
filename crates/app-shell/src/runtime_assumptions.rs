//! Spec 0101, §2 Frage 2: die eine Annahme über Tauris Async-Runtime,
//! auf der der synchrone Secret-Speicher steht — als Test, nicht als Satz
//! in einem Kommentar.
//!
//! `persistence_sqlite::SqliteCredentialStore` überbrückt den synchronen
//! `CredentialStore`-Trait zur asynchronen `sqlx`-API mit
//! `tokio::task::block_in_place` + `Handle::block_on`. `block_in_place`
//! **panickt** auf einer `current_thread`-Runtime („can call blocking only
//! when running on the multi-threaded runtime", gemessen).
//!
//! Der Store fängt das ab und gibt einen sichtbaren Fehler zurück, statt
//! vor dem ersten Fenster abzustürzen — aber dann funktionierte kein
//! einziges gespeichertes Passwort mehr. Dieser Test sorgt dafür, dass ein
//! Wechsel der Runtime-Konfiguration **hier** auffällt und nicht erst beim
//! Nutzer.

/// Tauris Async-Runtime ist eine Multi-Thread-Runtime. Ändert sich das,
/// scheitert dieser Test — und der Secret-Speicher braucht einen anderen
/// Weg aus dem synchronen Trait heraus (zweiter Pool, eigener Arbeitsthread
/// oder ein asynchroner Trait; alle drei sind Entscheidungen, die nicht
/// nebenbei fallen dürfen).
#[test]
fn test_spec_0101_the_tauri_async_runtime_is_multi_threaded() {
    let flavor = tauri::async_runtime::block_on(async {
        tokio::runtime::Handle::current().runtime_flavor()
    });

    assert_eq!(
        flavor,
        tokio::runtime::RuntimeFlavor::MultiThread,
        "der synchrone CredentialStore (Spec 0101, A9) blockiert mit \
         `block_in_place` einen Arbeitsthread — das geht nur auf einer \
         Multi-Thread-Runtime"
    );
}

/// Und die Gegenprobe zur zweiten Hälfte der Annahme: Beim Aufbau des
/// `AppState` läuft **keine** Runtime, `Handle::current()` wäre dort ein
/// Panic. Genau deshalb holt `build_app_state` den Griff innerhalb eines
/// `block_on` und reicht ihn dem Store hinein, statt ihn dort zu suchen.
#[test]
fn test_spec_0101_app_state_is_built_outside_the_runtime() {
    assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "ein Test läuft wie `build_app_state` außerhalb der Runtime — \
         wäre hier eine Runtime aktiv, prüfte der Test nichts"
    );
}
