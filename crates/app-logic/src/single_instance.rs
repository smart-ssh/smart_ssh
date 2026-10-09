//! Issue #44: Wann der Single-Instance-Schutz (zweiter Start holt die
//! laufende Instanz nach vorn, Issue #19) für einen Prozess gilt.
//!
//! Das Plugin dafür (`tauri-plugin-single-instance`) erkennt Instanzen nur
//! an der App-Kennung, nicht am Datenverzeichnis. Damit es nicht zwei
//! Instanzen mit **verschiedenen** Datenverzeichnissen gegeneinander
//! ausspielt, nimmt nur ein Prozess mit dem **Standard**-Datenverzeichnis
//! daran teil. Alle Teilnehmer teilen sich so dasselbe Verzeichnis, und ein
//! zweiter Teilnehmer scheitert immer schon an der Sperre auf das
//! Datenverzeichnis — bevor er die Datenbank anfasst. Ein Prozess mit
//! `SMART_SSH_DATA_DIR` nimmt nicht teil; ihn schützt allein die Sperre
//! (ADR 0106, ADR 0121).
//!
//! Hier steht nur die Entscheidung, ohne Tauri und ohne Umgebungszugriff;
//! `app-shell` liest Umgebung und Build-Art und registriert das Plugin.

/// Ob der Single-Instance-Schutz für diesen Prozess gilt.
///
/// - `raw_data_dir_override`: der Rohwert von `SMART_SSH_DATA_DIR`
///   (`None`, wenn nicht gesetzt). Ein leerer Wert gilt wie in
///   [`persistence_sqlite::default_db_path`] als nicht gesetzt.
/// - `debug_build`: Im Debug-Build ist der Schutz immer aus (ADR 0106 §6):
///   Debug- und Release-Build tragen dieselbe Kennung, aber getrennte
///   Standard-Datenverzeichnisse (ADR 0032).
pub fn focusing_applies(raw_data_dir_override: Option<&str>, debug_build: bool) -> bool {
    !debug_build && persistence_sqlite::data_dir_override_from(raw_data_dir_override).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_44_release_without_override_focuses() {
        assert!(focusing_applies(None, false));
    }

    #[test]
    fn test_issue_44_release_with_override_does_not_focus() {
        assert!(!focusing_applies(Some("/tmp/smart-ssh-b"), false));
    }

    #[test]
    fn test_issue_44_release_with_empty_override_is_treated_as_unset() {
        assert!(focusing_applies(Some(""), false));
    }

    #[test]
    fn test_issue_44_debug_build_never_focuses() {
        assert!(!focusing_applies(None, true));
        assert!(!focusing_applies(Some(""), true));
        assert!(!focusing_applies(Some("/tmp/smart-ssh-b"), true));
    }
}
