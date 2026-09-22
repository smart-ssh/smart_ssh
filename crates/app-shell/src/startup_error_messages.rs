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

/// Sprache der Startdialoge (Spec 0071, A11a). Bewusst nur zwei Werte: Der
/// Startdialog läuft **vor** der Tauri-Runtime und damit vor der
/// Frontend-`i18n`; er kann deren Übersetzungskatalog nicht benutzen und
/// trägt seine Texte selbst.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    De,
    En,
}

/// Spec 0071, A11a/A11c: Sprachwahl des Startdialogs als **reine** Funktion
/// über den bereits ausgewählten Umgebungswert — unit-testbar ohne
/// Umgebungsmanipulation, dieselbe Parameter-Injection wie bei
/// [`keychain_unavailable_text`]. Nur der Aufrufer in `crate::run` liest die
/// Variablen tatsächlich aus (s. [`preferred_locale_value`]).
///
/// Ausgewertet wird nur das Sprach-Präfix vor `_`, `.` oder `@`
/// (`de_DE.UTF-8` → `de`). Beginnt es mit `de` → Deutsch, sonst Englisch.
/// Ist nichts gesetzt oder der Wert unbrauchbar (`C`, `POSIX`, leer), gilt
/// **Deutsch** als Vorgabe — dasselbe Verhalten wie vor Spec 0071, damit ein
/// System ohne Locale-Einstellung nicht stillschweigend die Sprache wechselt.
///
/// `#[allow(dead_code)]`: in diesem Commit bewusst noch ohne Wirkung — die
/// Verdrahtung in `crate::run` folgt in §7 Schritt 5, damit der reine,
/// vollständig getestete Teil einzeln prüfbar bleibt.
#[allow(dead_code)]
pub fn startup_language(raw: Option<&str>) -> Language {
    let Some(raw) = raw else {
        return Language::De;
    };
    let prefix = raw
        .split(['_', '.', '@'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    // `C`/`POSIX` sind keine Sprachangaben, sondern die "keine Locale"-
    // Angabe von POSIX. Ohne diese Sonderbehandlung landeten sie in der
    // `else`-Hälfte unten und ergäben Englisch — die Spec verlangt hier
    // ausdrücklich die Vorgabe (A11a, T11a).
    if prefix.is_empty() || prefix == "c" || prefix == "posix" {
        return Language::De;
    }

    if prefix.starts_with("de") {
        Language::De
    } else {
        Language::En
    }
}

/// Spec 0071, A11a: die erste gesetzte, nicht leere Variable aus `LC_ALL`,
/// `LC_MESSAGES`, `LANG` — in genau dieser Reihenfolge (POSIX-Rangfolge:
/// `LC_ALL` überstimmt alles, `LANG` ist die schwächste Angabe).
///
/// Ebenfalls rein: Der Aufrufer liest die drei Variablen, diese Funktion
/// entscheidet nur, welche davon zählt.
///
/// `#[allow(dead_code)]`: s. [`startup_language`].
#[allow(dead_code)]
pub fn preferred_locale_value<'a>(
    lc_all: Option<&'a str>,
    lc_messages: Option<&'a str>,
    lang: Option<&'a str>,
) -> Option<&'a str> {
    [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
}

const CANNOT_START_TITLE: &str = "Smart SSH kann nicht starten";

/// spec-reviewer-Fund: ein Datenpfad enthält den Nutzer-Account-Namen (aus
/// `BaseDirs`), der theoretisch Steuerzeichen enthalten könnte — ohne diese
/// Bereinigung könnte ein `\n`/`\r` darin den Dialogtext optisch fortsetzen.
/// Ersetzt Steuerzeichen durch ein sichtbares `?`-Platzhalterzeichen statt
/// den Pfad stillschweigend zu kürzen (der volle Pfad bleibt für die
/// Fehlerdiagnose wichtig).
fn sanitize_path_for_display(path: &Path) -> String {
    path.display()
        .to_string()
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// Spec 0059, Fälle 1/2/4 — baut den Dialogtext aus einer bereits
/// klassifizierten [`ConnectFailureKind`] (s. `PersistenceError::classify`)
/// und dem Datenpfad, den `SqliteProfileStore::connect` versucht hat zu
/// öffnen. `log_dir`: spec-reviewer-Fund — der `Other`-Auffangfall (unten)
/// beschreibt eine Diagnose, die er nicht wirklich hat (er deckt auch
/// SQLITE_BUSY/gesperrte Datei, eine künftige fehlerhafte Migration, etc.
/// ab, nicht nur echte Korruption); der Logpfad gibt dem Nutzer/Support
/// wenigstens einen Weg zur echten Fehlerursache, ohne im Dialogtext selbst
/// zu spekulieren.
pub fn db_connect_failure_text(
    kind: &ConnectFailureKind,
    db_path: &Path,
    log_dir: &Path,
) -> DialogText {
    let db_path = sanitize_path_for_display(db_path);
    let log_dir = sanitize_path_for_display(log_dir);
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
             Datenpfad: {db_path}"
        ),
        // Spec 0059, Fall 4 (Datenverzeichnis nicht schreibbar).
        ConnectFailureKind::PermissionDenied => format!(
            "Das Datenverzeichnis von Smart SSH ist nicht beschreibbar oder lesbar \
             (Zugriff verweigert).\n\n\
             Bitte die Zugriffsrechte für dieses Verzeichnis prüfen.\n\n\
             Datenpfad: {db_path}"
        ),
        // Spec 0059, Fall 2 (DB korrupt/nicht lesbar) — zugleich der
        // generische Auffangfall für jeden anderen, nicht eigens benannten
        // Verbindungs-/Migrationsfehler (s. `ConnectFailureKind::Other`-
        // Doc-Kommentar in `persistence-sqlite`). spec-reviewer-Fund:
        // bewusst zurückhaltender formuliert ("konnte nicht geöffnet
        // werden — möglicherweise beschädigt oder von einem anderen
        // Programm gesperrt") statt pauschal "beschädigt" zu behaupten,
        // plus Verweis auf die Logdatei statt einer pauschalen
        // Backup-Empfehlung als erstem Schritt.
        ConnectFailureKind::Other => format!(
            "Die Datenbank von Smart SSH konnte nicht geöffnet werden — möglicherweise \
             beschädigt, oder von einem anderen laufenden Programm (z. B. einer zweiten \
             Instanz von Smart SSH) gesperrt.\n\n\
             Nächster Schritt: prüfe, ob eine andere Instanz von Smart SSH läuft und \
             beende sie; Details zur genauen Ursache stehen im Log unter {log_dir}. \
             Hilft das nicht weiter, kann ein vorhandenes Backup der Datenbank eingespielt \
             oder der Smart-SSH-Support kontaktiert werden.\n\n\
             Datenpfad: {db_path}"
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
    let path = sanitize_path_for_display(path);
    DialogText {
        title: CANNOT_START_TITLE.to_string(),
        message: format!(
            "Der Host-Key-Speicher von Smart SSH konnte nicht geladen werden (Datei \
             beschädigt oder Zugriffsproblem).\n\n\
             Bitte die Zugriffsrechte für dieses Verzeichnis prüfen. Ist die Datei \
             beschädigt, kann sie gelöscht werden — bereits bekannte Server-Fingerabdrücke \
             müssen dann beim nächsten Verbindungsaufbau erneut bestätigt werden. \
             Achtung: eine erneute Erstbestätigung bietet KEINEN Schutz vor einem seither \
             untergeschobenen Server — prüfe unbekannte Fingerabdrücke nach dem Löschen \
             gegen eine vertrauenswürdige Quelle (z. B. beim Serverbetreiber nachfragen), \
             statt sie blind zu bestätigen.\n\n\
             Datenpfad: {path}"
        ),
    }
}

/// spec-reviewer-Fund: die Entscheidung "welcher `CipherError` löst die
/// sichtbare Fall-3-Warnung aus" stand bisher nur als `matches!(...)` direkt
/// in `crate::run` — ohne echte Keychain/DB ist dieser Aufrufort selbst
/// nicht unit-testbar. Als eigene, reine Funktion hier lässt sich die
/// Abgrenzung (nur `KeyStoreAccessFailed`, NICHT `InvalidKey` — s. Spec
/// 0059, Fall 3 vs. Spec 0040 Abschnitt 7) unabhängig davon festnageln.
pub fn should_warn_about_keychain(err: &ssh_manager_core::crypto::CipherError) -> bool {
    matches!(
        err,
        ssh_manager_core::crypto::CipherError::KeyStoreAccessFailed(_)
    )
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

    /// Spec 0071, T11a — die vollständige Tabelle aus A11a, inklusive der
    /// Vorgabe-Fälle. `C`/`POSIX` sind der eigentliche Stolperstein: Sie
    /// beginnen nicht mit `de` und ergäben ohne Sonderbehandlung Englisch,
    /// obwohl die Spec dort die Vorgabe (Deutsch) verlangt.
    #[test]
    fn test_startup_language_reads_only_the_language_prefix_and_defaults_to_german() {
        for raw in ["de", "de_DE.UTF-8", "de_AT@euro", "de_CH", "DE_DE.UTF-8"] {
            assert_eq!(
                startup_language(Some(raw)),
                Language::De,
                "{raw} muss Deutsch ergeben"
            );
        }
        for raw in ["en_US.UTF-8", "fr_FR", "ja_JP.UTF-8", "en", "nl_NL"] {
            assert_eq!(
                startup_language(Some(raw)),
                Language::En,
                "{raw} muss Englisch ergeben"
            );
        }
        for raw in ["C", "POSIX", "C.UTF-8", "", "   "] {
            assert_eq!(
                startup_language(Some(raw)),
                Language::De,
                "{raw:?} ist keine brauchbare Sprachangabe und muss auf die Vorgabe fallen"
            );
        }
        assert_eq!(
            startup_language(None),
            Language::De,
            "keine Variable gesetzt → Vorgabe"
        );
    }

    /// Spec 0071, A11a: POSIX-Rangfolge `LC_ALL` > `LC_MESSAGES` > `LANG`,
    /// wobei eine gesetzte, aber leere Variable übersprungen wird (sonst
    /// würde ein `LC_ALL=""` die tatsächlich gesetzte `LANG` verdecken).
    #[test]
    fn test_preferred_locale_value_follows_the_posix_precedence() {
        assert_eq!(
            preferred_locale_value(Some("en_US.UTF-8"), Some("de_DE"), Some("fr_FR")),
            Some("en_US.UTF-8")
        );
        assert_eq!(
            preferred_locale_value(None, Some("de_DE"), Some("fr_FR")),
            Some("de_DE")
        );
        assert_eq!(
            preferred_locale_value(None, None, Some("fr_FR")),
            Some("fr_FR")
        );
        assert_eq!(preferred_locale_value(None, None, None), None);
        assert_eq!(
            preferred_locale_value(Some(""), Some("  "), Some("en_GB")),
            Some("en_GB"),
            "gesetzt, aber leer darf die nächste Variable nicht verdecken"
        );
    }

    #[test]
    fn test_schema_too_new_message_contains_both_version_numbers_and_the_path() {
        let text = db_connect_failure_text(
            &ConnectFailureKind::SchemaTooNew {
                applied_version: 15,
                max_known_version: 12,
            },
            Path::new("/tmp/test/smart-ssh.db"),
            Path::new("/tmp/test/logs"),
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
            Path::new("/tmp/test/logs"),
        );
        assert!(text.message.contains("/tmp/test/smart-ssh.db"));
        assert!(text.message.to_lowercase().contains("zugriffsrechte"));
    }

    /// spec-reviewer-Fund: der `Other`-Auffangfall deckt auch Fälle wie
    /// SQLITE_BUSY (gesperrt durch eine zweite laufende Instanz) ab, nicht
    /// nur echte Korruption — der Text darf deshalb keine pauschale
    /// "beschädigt, Backup einspielen"-Diagnose mehr behaupten, sondern
    /// muss auf die Logdatei verweisen und Backup/Support erst als
    /// nachrangigen Schritt nennen.
    #[test]
    fn test_other_db_failure_message_is_cautious_and_points_to_the_log() {
        let text = db_connect_failure_text(
            &ConnectFailureKind::Other,
            Path::new("/tmp/test/smart-ssh.db"),
            Path::new("/tmp/test/logs"),
        );
        assert!(text.message.contains("/tmp/test/smart-ssh.db"));
        assert!(text.message.contains("/tmp/test/logs"));
        assert!(text.message.contains("Backup"));
        assert!(text.message.contains("Support"));
        assert!(
            text.message.contains("möglicherweise"),
            "darf Korruption nicht als sichere Diagnose behaupten: {}",
            text.message
        );
        assert!(
            text.message.to_lowercase().contains("andere instanz")
                || text.message.to_lowercase().contains("gesperrt"),
            "muss den häufigen Fall einer zweiten laufenden Instanz nennen: {}",
            text.message
        );
    }

    #[test]
    fn test_host_key_store_failure_message_mentions_the_path() {
        let text = host_key_store_failure_text(Path::new("/tmp/test/host_keys.json"));
        assert!(text.message.contains("/tmp/test/host_keys.json"));
    }

    /// spec-reviewer-Fund: das Löschen einer beschädigten Host-Key-Datei
    /// wirft die gesamte TOFU-Pinning-Historie weg — der Text muss das
    /// nicht nur technisch erwähnen, sondern als Risiko kennzeichnen,
    /// sonst wirkt ein anschließender MITM-Server wie ein harmloser "neuer
    /// Server, bitte bestätigen".
    #[test]
    fn test_host_key_store_failure_message_warns_about_deleting_the_file() {
        let text = host_key_store_failure_text(Path::new("/tmp/test/host_keys.json"));
        assert!(
            text.message.to_lowercase().contains("kein")
                && text.message.to_lowercase().contains("schutz"),
            "muss ausdrücklich sagen, dass Löschen KEINEN Schutz vor einem untergeschobenen \
             Server bietet: {}",
            text.message
        );
    }

    #[test]
    fn test_sanitize_path_for_display_replaces_control_characters() {
        let text = host_key_store_failure_text(Path::new("/tmp/evil\nBitte Passwort senden"));
        assert!(
            !text
                .message
                .lines()
                .any(|line| line.trim() == "Bitte Passwort senden"),
            "ein Steuerzeichen im Pfad darf den Dialogtext nicht optisch fortsetzen: {}",
            text.message
        );
    }

    /// spec-reviewer-Fund: die Fall-3-vs-Spec-0040-Abgrenzung (nur ein
    /// echter Zugriffsfehler ist sichtbar, ein korrupter Schlüsselwert
    /// bleibt beim stillen `tracing::warn!`) war zuvor nur an der
    /// Aufrufstelle in `crate::run` geprüft, dort ohne echte
    /// Keychain/DB nicht testbar.
    #[test]
    fn test_should_warn_about_keychain_only_for_the_access_error_not_a_corrupt_key() {
        use ssh_manager_core::crypto::CipherError;
        assert!(should_warn_about_keychain(
            &CipherError::KeyStoreAccessFailed("locked".to_string())
        ));
        assert!(!should_warn_about_keychain(&CipherError::InvalidKey));
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
                Path::new("/tmp/logs"),
            ),
            db_connect_failure_text(
                &ConnectFailureKind::PermissionDenied,
                Path::new("/tmp/db"),
                Path::new("/tmp/logs"),
            ),
            db_connect_failure_text(
                &ConnectFailureKind::Other,
                Path::new("/tmp/db"),
                Path::new("/tmp/logs"),
            ),
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
