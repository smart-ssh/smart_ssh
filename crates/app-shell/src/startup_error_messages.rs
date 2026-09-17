//! Baut die Nutzer-sichtbaren Texte für die vier Startup-Fehlerfälle (Spec
//! 0059) — bewusst getrennt von `crate::startup_dialog` (dem eigentlichen
//! GUI-Aufruf): die "richtiger Text für den richtigen Fall"-Logik ist so
//! ohne jede GUI-/`rfd`-Abhängigkeit unit-testbar (Spec 0059, Testbarkeit:
//! "die Fehlererkennung + der Dialog-Aufruf mit richtigem Text unit-testbar
//! machen — die visuelle Bestätigung macht Stefan pro Plattform").
//!
//! **Invariante (Spec 0059)**: kein Secret/kein DB-Inhalt in einem dieser
//! Texte — nur Fehlerart, Datenpfad, nächster Schritt. Jede Funktion hier
//! nimmt bewusst nur genau die Daten entgegen, die für den jeweiligen Text
//! nötig sind (nie einen rohen Fehler/eine rohe Exception mit
//! möglicherweise sensiblem Inhalt durchgereicht).

use std::path::Path;

use persistence_sqlite::ConnectFailureKind;

pub struct DialogText {
    pub title: String,
    pub message: String,
}

const CANNOT_START_TITLE: &str = "Smart SSH kann nicht starten";

/// Spec 0059, Fälle 1/2/4 — baut den Dialogtext aus einer bereits
/// klassifizierten [`ConnectFailureKind`] (s. `PersistenceError::classify`)
/// und dem Datenpfad, den `SqliteProfileStore::connect` versucht hat zu
/// öffnen.
pub fn db_connect_failure_text(kind: &ConnectFailureKind, db_path: &Path) -> DialogText {
    let message = match kind {
        // Spec 0059, Fall 1 (DB-Downgrade): Versionsnummern wörtlich wie
        // in der Aufgabenstellung gefordert ("angelegt mit X, dieses
        // Programm kennt bis Y").
        ConnectFailureKind::SchemaTooNew {
            applied_version,
            max_known_version,
        } => format!(
            "Die Datenbank wurde von einer neueren Version von Smart SSH angelegt \
             (Datenbank-Version {applied_version}, dieses Programm kennt Versionen bis \
             {max_known_version}).\n\n\
             Bitte installiere die neueste Version von Smart SSH.\n\n\
             Datenpfad: {}",
            db_path.display()
        ),
        // Spec 0059, Fall 4 (Datenverzeichnis nicht schreibbar).
        ConnectFailureKind::PermissionDenied => format!(
            "Das Datenverzeichnis von Smart SSH ist nicht beschreibbar oder lesbar \
             (Zugriff verweigert).\n\n\
             Bitte die Zugriffsrechte für dieses Verzeichnis prüfen.\n\n\
             Datenpfad: {}",
            db_path.display()
        ),
        // Spec 0059, Fall 2 (DB korrupt/nicht lesbar) — zugleich der
        // generische Auffangfall für jeden anderen, nicht eigens benannten
        // Verbindungs-/Migrationsfehler (s. `ConnectFailureKind::Other`-
        // Doc-Kommentar in `persistence-sqlite`): "Datenbank beschädigt"
        // ist die sichere, für einen Nutzer verständliche Beschreibung für
        // "beim Öffnen/Migrieren ist etwas Unerwartetes schiefgegangen".
        ConnectFailureKind::Other => format!(
            "Die Datenbank von Smart SSH ist beschädigt oder nicht lesbar und konnte nicht \
             geöffnet werden.\n\n\
             Nächster Schritt: ein vorhandenes Backup der Datenbank einspielen oder den \
             Smart-SSH-Support kontaktieren.\n\n\
             Datenpfad: {}",
            db_path.display()
        ),
    };
    DialogText {
        title: CANNOT_START_TITLE.to_string(),
        message,
    }
}

/// Nicht einer der vier in Spec 0059 namentlich benannten Fälle, aber auf
/// demselben kritischen Pfad und derselben Fehlerklasse wie Fall 4
/// (Datenverzeichnis-Zugriffsproblem) — `FileHostKeyStore::load` teilt
/// sich dasselbe Datenverzeichnis wie die SQLite-Datenbank (s.
/// `crate::run`) und hatte vor diesem Schritt ein eigenes, unbehandeltes
/// `.expect(...)`. Mit demselben Mechanismus geschlossen, statt eines
/// bekannten, dokumentierten Panics direkt neben den vier behobenen
/// Fällen.
pub fn host_key_store_failure_text(path: &Path) -> DialogText {
    DialogText {
        title: CANNOT_START_TITLE.to_string(),
        message: format!(
            "Der Host-Key-Speicher von Smart SSH konnte nicht geladen werden (Datei \
             beschädigt oder Zugriffsproblem).\n\n\
             Bitte die Zugriffsrechte für dieses Verzeichnis prüfen. Ist die Datei \
             beschädigt, kann sie gelöscht werden — bereits bekannte Server-Fingerabdrücke \
             müssen dann beim nächsten Verbindungsaufbau erneut bestätigt werden.\n\n\
             Datenpfad: {}",
            path.display()
        ),
    }
}

/// Spec 0059, Fall 3 (Keychain/Secret-Service beim Start gesperrt oder
/// nicht vorhanden) — nicht-fatal (s. `crate::startup_dialog::
/// show_warning`-Doc-Kommentar): nennt auf Linux ausdrücklich, welches
/// Paket fehlen könnte (Spec 0059, Fall 3, wörtlich).
///
/// `target_os` statt `#[cfg(target_os = "linux")]`: reine Parameter-
/// Injection, damit BEIDE Zweige unabhängig vom tatsächlichen Build-Ziel
/// unit-testbar sind (sonst ließe sich der Linux-Zweig nur auf einer
/// Linux-CI/-Maschine ausführen) — Aufrufer übergibt `std::env::consts::
/// OS`.
pub fn keychain_unavailable_text(target_os: &str) -> DialogText {
    let mut message = "Der Systemschlüsselbund konnte nicht geöffnet werden.\n\n\
         Chat-Verlauf, Notiz-Zusammenfassungen und die Eingabe-Historie sind für diesen \
         Programmlauf deaktiviert. Alle anderen Funktionen (SSH-Verbindungen, KI-Chat, \
         Filter-Regeln) funktionieren normal — Smart SSH wird jetzt trotzdem gestartet."
        .to_string();
    if target_os == "linux" {
        message.push_str(
            "\n\nAuf Linux prüft das meist: läuft ein Secret-Service-Anbieter (z. B. \
             gnome-keyring, KWallet oder KeePassXC mit aktivierter Secret-Service-\
             Integration)? Ist er gesperrt?",
        );
    }
    DialogText {
        title: "Smart SSH: eingeschränkter Start".to_string(),
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_too_new_message_contains_both_version_numbers_and_the_path() {
        let text = db_connect_failure_text(
            &ConnectFailureKind::SchemaTooNew {
                applied_version: 15,
                max_known_version: 12,
            },
            Path::new("/tmp/test/smart-ssh.db"),
        );
        assert!(text.message.contains("15"));
        assert!(text.message.contains("12"));
        assert!(text.message.contains("/tmp/test/smart-ssh.db"));
        assert!(
            text.message.contains("neueste Version"),
            "muss den nächsten Schritt (Update installieren) nennen: {}",
            text.message
        );
    }

    #[test]
    fn test_permission_denied_message_mentions_the_path_and_permissions() {
        let text = db_connect_failure_text(
            &ConnectFailureKind::PermissionDenied,
            Path::new("/tmp/test/smart-ssh.db"),
        );
        assert!(text.message.contains("/tmp/test/smart-ssh.db"));
        assert!(text.message.to_lowercase().contains("zugriffsrechte"));
    }

    #[test]
    fn test_other_db_failure_message_mentions_backup_and_support() {
        let text = db_connect_failure_text(
            &ConnectFailureKind::Other,
            Path::new("/tmp/test/smart-ssh.db"),
        );
        assert!(text.message.contains("/tmp/test/smart-ssh.db"));
        assert!(text.message.contains("Backup"));
        assert!(text.message.contains("Support"));
    }

    #[test]
    fn test_host_key_store_failure_message_mentions_the_path() {
        let text = host_key_store_failure_text(Path::new("/tmp/test/host_keys.json"));
        assert!(text.message.contains("/tmp/test/host_keys.json"));
    }

    #[test]
    fn test_keychain_message_names_linux_secret_service_packages_only_on_linux() {
        let linux_text = keychain_unavailable_text("linux");
        assert!(linux_text.message.contains("gnome-keyring"));
        assert!(linux_text.message.contains("KWallet"));
        assert!(linux_text.message.contains("KeePassXC"));

        let macos_text = keychain_unavailable_text("macos");
        assert!(!macos_text.message.contains("gnome-keyring"));

        let windows_text = keychain_unavailable_text("windows");
        assert!(!windows_text.message.contains("gnome-keyring"));
    }

    #[test]
    fn test_keychain_message_is_non_fatal_in_tone_and_explains_what_still_works() {
        let text = keychain_unavailable_text("macos");
        assert!(
            text.message.contains("trotzdem gestartet"),
            "der Text muss klarstellen, dass die App weiterläuft, nicht abbricht: {}",
            text.message
        );
        assert!(text.message.contains("SSH-Verbindungen"));
    }

    /// Spec 0059, Invarianten: "kein Secret/kein sensibler Inhalt im
    /// Dialog-Text". Keine dieser Funktionen nimmt überhaupt einen rohen
    /// Fehler/DB-Inhalt als Parameter entgegen (nur `ConnectFailureKind`,
    /// `Path`, `&str`) — dieser Test dokumentiert die Invariante trotzdem
    /// exekutierbar: keiner der erzeugten Texte enthält versehentlich
    /// einen Platzhalter-"Secret"-artigen String.
    #[test]
    fn test_no_generated_text_leaks_a_placeholder_secret_value() {
        let texts = [
            db_connect_failure_text(
                &ConnectFailureKind::SchemaTooNew {
                    applied_version: 1,
                    max_known_version: 1,
                },
                Path::new("/tmp/db"),
            ),
            db_connect_failure_text(&ConnectFailureKind::PermissionDenied, Path::new("/tmp/db")),
            db_connect_failure_text(&ConnectFailureKind::Other, Path::new("/tmp/db")),
            host_key_store_failure_text(Path::new("/tmp/host_keys.json")),
            keychain_unavailable_text("linux"),
        ];
        for text in texts {
            assert!(!text.message.contains("hunter2"));
            assert!(!text.message.contains("sk-"));
            assert!(!text.message.to_lowercase().contains("password="));
        }
    }
}
