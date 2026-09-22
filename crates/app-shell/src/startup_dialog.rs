//! Nativer, Tauri-unabhängiger Fehlerdialog für fatale Startfehler (Spec
//! 0059) — läuft **vor** `tauri::Builder::default()`, wenn noch kein
//! Fenster/keine Tauri-Runtime existiert. Vier Startup-Fehlerfälle
//! (Release-Gate A) hatten bisher keinen Fehlerpfad: ein `.expect()`-Panic
//! vor dem ersten Fenster zeigt für einen Doppelklick-Nutzer nur ein
//! kurzes, undiagnostizierbares Aufblitzen ohne jedes Fenster.
//!
//! **Mechanismus: `rfd::MessageDialog`, nicht `tauri_plugin_dialog`** (Spec
//! 0059, Teil 0/1 — im echten Crate-Kontext geprüft, nicht aus der Doku
//! vermutet):
//! - `tauri-plugin-dialog` (bereits eine direkte `app-shell`-Abhängigkeit
//!   für die regulären In-App-Dialoge, z. B. Datei-/Ordnerauswahl) wickelt
//!   `rfd` intern über einen laufenden `tauri::AppHandle` — genau die
//!   Voraussetzung, die an dieser Stelle noch nicht existiert.
//! - `rfd` selbst (Version 0.16, bereits **transitiv** über `tauri-plugin-
//!   dialog` im Dependency-Baum, s. `Cargo.lock` — hier nur zu einer
//!   direkten Abhängigkeit hochgestuft) lässt sich davon unabhängig
//!   aufrufen: `rfd::MessageDialog::new()....show()` baut/zeigt den
//!   Dialog vollständig selbst (macOS: `NSAlert::runModal()`, ein
//!   eigenständiger modaler Run-Loop; Linux/GTK3: ein von `tauri-plugin-
//!   dialog` bereits standardmäßig genutzter, eigener GTK-Thread; Windows:
//!   `MessageBox`/`TaskDialogIndirect`) — blockiert synchron, ganz ohne
//!   vorher existierendes Fenster oder laufende Tauri-/Webview-Runtime.
//! - Keine dritte, plattformspezifische `#[cfg(...)]`-Lösung nötig — genau
//!   die von Spec 0059 Teil 0 bevorzugte "eine plattformübergreifende
//!   Crate, falls eine ohne Fenster-System funktioniert"-Option.
//!
//! **Logging zuerst, Dialog zusätzlich** (Spec 0047, Fund B1 / Spec 0059,
//! Teil 1): beide Funktionen hier loggen strukturiert, BEVOR der Dialog
//! erscheint — der 0047-B1-Logger läuft bereits als Allererstes in
//! `crate::run`, dieser Dialog ist die zusätzliche, sichtbare Ergänzung,
//! nicht der Ersatz.

use rfd::{MessageButtons, MessageDialog, MessageLevel};

/// Zeigt einen fatalen Fehlerdialog und beendet den Prozess danach sauber
/// — nie ein Zurückkehren in einen halb aufgebauten/kaputten Zustand (Spec
/// 0059, Invarianten: "Dialog ändert nichts an den Daten … nur melden +
/// beenden"). `std::process::exit` statt `panic!`: ein Panic würde (a) den
/// gerade erst installierten Panic-Hook erneut durchlaufen (verwirrende
/// doppelte Log-Zeile) und (b) auf manchen Plattformen einen zusätzlichen
/// Betriebssystem-Crash-Reporter/-Dialog auslösen — genau das
/// undiagnostizierbare, technische "Aufblitzen", das dieser Mechanismus
/// gerade ersetzen soll. Exit-Code `1`: dieselbe Konvention wie ein
/// fehlgeschlagener `.run(context).expect(...)` am Ende von `crate::run`.
pub fn show_fatal_error_and_exit(title: &str, message: &str) -> ! {
    tracing::error!(title, message, "showing fatal startup error dialog");
    MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_level(MessageLevel::Error)
        .set_buttons(MessageButtons::Ok)
        .show();
    std::process::exit(1);
}

/// Zeigt eine SICHTBARE, aber nicht-fatale Warnung — die App läuft danach
/// unverändert weiter. Spec 0059, Fall 3 (Keychain/Secret-Service gesperrt
/// oder fehlt): bewusste Entscheidung, die Spec-0040-Design-Entscheidung zu
/// bewahren, dass ein Keychain-Problem den App-**Start** nicht verhindert —
/// der Dialog macht das bisher stille `tracing::warn!` nur zusätzlich
/// SICHTBAR, statt die App abzubrechen.
///
/// **Korrektur durch Spec 0071 (spec-reviewer-Fund):** Hier stand bis dahin,
/// ein Keychain-Problem betreffe „nur EINE optionale Komfortfunktion
/// (Chat-Persistenz/Prompt-Historie/Ledger), nicht den App-Kern
/// (SSH-Verbindungen, KI-Chat, Filter-Engine funktionieren unverändert)".
/// Das ist falsch und war der Kern von BL-0031: Ohne Schlüsselbund
/// scheitert **jeder** Zugriff auf den `CredentialStore`, also lässt sich
/// kein KI-Provider anlegen (und damit gibt es keinen KI-Chat), kein
/// Server-Passwort, keine Passphrase und kein Sudo-Passwort speichern oder
/// lesen. Unverändert funktionieren nur SSH-Verbindungen über den
/// SSH-Agent oder mit einem Schlüssel ohne Passphrase. Nicht-fatal heißt
/// also „die App startet", nicht „alles andere geht".
pub fn show_warning(title: &str, message: &str) {
    tracing::warn!(title, message, "showing non-fatal startup warning dialog");
    MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_level(MessageLevel::Warning)
        .set_buttons(MessageButtons::Ok)
        .show();
}
