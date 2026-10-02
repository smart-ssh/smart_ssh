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
///
/// **Seit Spec 0101 (A3) nicht mehr aufgerufen, und das ist kein
/// Versehen.** Der Fall, für den diese Warnung gebaut wurde — der
/// Schlüsselbund ist nicht erreichbar, die App läuft trotzdem, nur ohne
/// Chat-Persistenz —, existiert nicht mehr: Ohne K lässt sich die
/// verschlüsselte Datenbank gar nicht öffnen, und der Start endet in Dialog
/// D1 („Erneut versuchen" / „Beenden"). Das ist strenger als vorher, nicht
/// nachlässiger: Es gibt keinen halb benutzbaren Zustand mehr, in dem
/// unklar bleibt, was gerade geschrieben wird und was nicht.
///
/// Die Funktion bleibt stehen, weil Etappe 3 (A11.1, A17) wieder
/// nicht-fatale Hinweise beim Start braucht — und ein zweites Mal denselben
/// `rfd`-Aufruf zu schreiben wäre die schlechtere Lösung als ein `allow`.
#[allow(dead_code)]
pub fn show_warning(title: &str, message: &str) {
    tracing::warn!(title, message, "showing non-fatal startup warning dialog");
    MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_level(MessageLevel::Warning)
        .set_buttons(MessageButtons::Ok)
        .show();
}

/// Welchen Knopf der Nutzer in einem Dialog mit Wahl gedrückt hat (Spec
/// 0101, D1–D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogAnswer {
    /// Der Knopf, der die Handlung auslöst.
    Confirm,
    /// Der Knopf, der nichts tut — **und jeder andere Ausgang**, s. [`ask`].
    Cancel,
    /// Der dritte Knopf, falls der Dialog einen hatte.
    Extra,
}

/// Spec 0101, A3/A5: ein nativer Dialog mit zwei oder drei beschrifteten
/// Knöpfen, vor der Tauri-Runtime — derselbe `rfd`-Mechanismus wie
/// [`show_fatal_error_and_exit`] (s. Moduldoku), nur mit eigenen
/// Beschriftungen statt eines bloßen „OK".
///
/// **Jeder nicht eindeutige Ausgang gilt als [`DialogAnswer::Cancel`]**
/// (Dialog mit Escape geschlossen, Fenstermanager schließt ihn, eine
/// künftige `rfd`-Fassung liefert etwas Unerwartetes). Das ist die sichere
/// Richtung: `Cancel` heißt an jeder Aufrufstelle „nichts anfassen". Ein
/// Rückfall auf `Confirm` würde eine Datei umbenennen oder einen Schlüssel
/// ersetzen, ohne dass jemand zugestimmt hat.
pub fn ask(
    title: &str,
    message: &str,
    confirm: &str,
    cancel: &str,
    extra: Option<&str>,
) -> DialogAnswer {
    tracing::info!(title, "showing startup choice dialog");
    let buttons = match extra {
        Some(extra) => MessageButtons::YesNoCancelCustom(
            confirm.to_string(),
            extra.to_string(),
            cancel.to_string(),
        ),
        None => MessageButtons::OkCancelCustom(confirm.to_string(), cancel.to_string()),
    };
    let result = MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_level(MessageLevel::Warning)
        .set_buttons(buttons)
        .show();
    let answer = match &result {
        rfd::MessageDialogResult::Custom(label) if label == confirm => DialogAnswer::Confirm,
        rfd::MessageDialogResult::Custom(label) if Some(label.as_str()) == extra => {
            DialogAnswer::Extra
        }
        _ => DialogAnswer::Cancel,
    };
    tracing::info!(?answer, "startup choice dialog answered");
    answer
}

/// Spec 0101, A5: meldet nach dem Umbenennen den neuen Dateinamen. Reine
/// Mitteilung, kein Fehler — deshalb `MessageLevel::Info` statt des
/// Warn-Stils von [`show_warning`].
pub fn show_info(title: &str, message: &str) {
    tracing::info!(title, message, "showing startup info dialog");
    MessageDialog::new()
        .set_title(title)
        .set_description(message)
        .set_level(MessageLevel::Info)
        .set_buttons(MessageButtons::Ok)
        .show();
}
