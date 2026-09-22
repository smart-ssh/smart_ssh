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

use credentials_keyring::KeychainUnavailableReason;
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

/// Spec 0071, A11b: Die Sprachwahl gilt für **alle** Startdialog-Texte, also
/// auch für die Titelzeile — ein englischsprachiges System darf nicht einen
/// deutschen Titel über einer englischen Meldung zeigen.
fn cannot_start_title(language: Language) -> &'static str {
    match language {
        Language::De => "Smart SSH kann nicht starten",
        Language::En => "Smart SSH cannot start",
    }
}

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
///
/// Spec 0071, A11b: zusätzlich sprachabhängig. Die englischen Fassungen sind
/// **Übersetzungen**, keine Neufassungen — jede nennt dieselbe Ursache,
/// denselben nächsten Schritt und denselben Datenpfad wie die deutsche.
pub fn db_connect_failure_text(
    kind: &ConnectFailureKind,
    db_path: &Path,
    log_dir: &Path,
    language: Language,
) -> DialogText {
    let db_path = sanitize_path_for_display(db_path);
    let log_dir = sanitize_path_for_display(log_dir);
    let message = match (language, kind) {
        (
            Language::En,
            ConnectFailureKind::SchemaTooNew {
                applied_version,
                max_known_version,
            },
        ) => format!(
            "The database was created by a newer version of Smart SSH (database version \
             {applied_version}, this build knows versions up to {max_known_version}).\n\n\
             Please install the latest version of Smart SSH.\n\n\
             Data path: {db_path}"
        ),
        (Language::En, ConnectFailureKind::PermissionDenied) => format!(
            "Smart SSH's data directory cannot be read from or written to (access \
             denied).\n\n\
             Please check the access permissions for this directory.\n\n\
             Data path: {db_path}"
        ),
        (Language::En, ConnectFailureKind::Other) => format!(
            "Smart SSH's database could not be opened — it may be damaged, or locked by \
             another running program (for example a second instance of Smart SSH).\n\n\
             Next step: check whether another instance of Smart SSH is running and quit it; \
             details on the exact cause are in the log under {log_dir}. If that does not \
             help, an existing Backup of the database can be restored, or Smart SSH Support \
             can be contacted.\n\n\
             Data path: {db_path}"
        ),
        (Language::De, kind) => db_connect_failure_message_de(kind, &db_path, &log_dir),
    };
    DialogText {
        title: cannot_start_title(language).to_string(),
        message,
    }
}

fn db_connect_failure_message_de(
    kind: &ConnectFailureKind,
    db_path: &str,
    log_dir: &str,
) -> String {
    match kind {
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
///
/// Spec 0071, A11b/T11c: Die englische Fassung trägt die Sicherheitswarnung
/// **ebenso ausdrücklich** wie die deutsche. Dieser Text steht auf einem
/// Pfad, an dem ein Nutzer die gesamte TOFU-Pinning-Historie wegwirft; eine
/// abgeschwächte Übersetzung wäre hier kein Schönheitsfehler, sondern ein
/// Sicherheitsfehler.
pub fn host_key_store_failure_text(path: &Path, language: Language) -> DialogText {
    let path = sanitize_path_for_display(path);
    let message = match language {
        Language::De => format!(
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
        Language::En => format!(
            "Smart SSH's host-key store could not be loaded (file damaged or access \
             problem).\n\n\
             Please check the access permissions for this directory. If the file is \
             damaged it can be deleted — already known server fingerprints then have to be \
             confirmed again on the next connection. Careful: confirming again offers NO \
             protection against a server that was swapped out in the meantime — after \
             deleting, check unknown fingerprints against a trustworthy source (for example \
             by asking the server operator) instead of confirming them blindly.\n\n\
             Data path: {path}"
        ),
    };
    DialogText {
        title: cannot_start_title(language).to_string(),
        message,
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

/// Spec 0071, A5 (b): die Aufzählung der **blockierten** Funktionen —
/// derselbe Absatz für jeden Grund, weil die Folgen identisch sind: Ohne
/// Schlüsselbund scheitert jeder Schreib- und Lesezugriff auf den
/// `CredentialStore`.
///
/// Ersetzt den Satz "Alle anderen Funktionen (SSH-Verbindungen, KI-Chat,
/// Filter-Regeln) funktionieren normal" aus Spec 0059 (A9). Der war im
/// BL-0031-Fall schlicht falsch: `add_ai_provider` schreibt den API-Key
/// unbedingt vor der DB-Zeile, der Provider lässt sich also gar nicht
/// anlegen — und ohne Provider gibt es keinen KI-Chat.
fn keychain_blocked_functions(language: Language) -> &'static str {
    match language {
        Language::De => {
            "Solange das so ist, lassen sich kein API-Key für einen KI-Provider, kein \
             Server-Passwort, keine Passphrase und kein Sudo-Passwort speichern oder lesen. \
             Chat-Verlauf, Notiz-Zusammenfassungen und die Eingabe-Historie sind für diesen \
             Programmlauf deaktiviert. SSH-Verbindungen über den SSH-Agent oder mit einem \
             Schlüssel ohne Passphrase funktionieren weiterhin."
        }
        Language::En => {
            "While this is the case, no API key for an AI provider, no server password, no \
             passphrase and no sudo password can be saved or read. Chat history, note \
             summaries and the input history are disabled for this app run. SSH connections \
             through the SSH agent, or with a key that has no passphrase, keep working."
        }
    }
}

/// Spec 0071, A9: Die Nicht-Fatalität aus Spec 0059 bleibt unverändert —
/// diese Spec ändert nur, *was* gemeldet wird, nicht *ob* gestartet wird.
fn keychain_still_starts(language: Language) -> &'static str {
    match language {
        Language::De => "Smart SSH wird jetzt trotzdem gestartet.",
        Language::En => "Smart SSH will still start.",
    }
}

/// Spec 0071, A5–A9: der Startdialog-Text zu einem klassifizierten
/// [`KeychainUnavailableReason`]. Jeder Text nennt (a) den Zustand in einem
/// Satz, (b) die blockierten Funktionen und (c) genau einen nächsten Schritt.
///
/// `target_os` bleibt wie vor Spec 0071 ein Parameter statt
/// `#[cfg(target_os = "linux")]`, damit alle Zweige unabhängig vom
/// tatsächlichen Build-Ziel unit-testbar sind — der Aufrufer übergibt
/// `std::env::consts::OS`.
///
/// **A8, per Konstruktion:** Außerhalb von Linux wird *jeder* Grund auf den
/// OS-neutralen Auffangtext abgebildet. Der Klassifizierer liefert dort
/// ohnehin nur [`KeychainUnavailableReason::Unknown`]
/// (`credentials_keyring::classify_store_failure`), aber so kann auch ein
/// künftiger Aufrufer keinen `apt`-Befehl nach macOS oder Windows tragen.
/// Das ist kein stiller Rückfall auf etwas Schwächeres: Der Auffangtext ist
/// nach A4 vollständig, er nennt nur keine Linux-Pakete.
pub fn keychain_unavailable_text(
    reason: KeychainUnavailableReason,
    target_os: &str,
    language: Language,
) -> DialogText {
    let linux = target_os == "linux";
    let blocked = keychain_blocked_functions(language);
    let still_starts = keychain_still_starts(language);

    let (title, state, next_step) = match (linux, reason, language) {
        // ── Kein Anbieter (Linux) ──────────────────────────────────────
        // ANNAHME A-1: Die Paketnamen sind aus §1.1/A5 der Spec übernommen,
        // nicht gemessen — der Container-Teil von Teil 0 (A0.3) war im
        // Coder-Lauf nicht durchführbar (Entscheidung vom 2026-09-22,
        // festgehalten als Klarstellung in §9 der Spec). Vor dem Merge
        // durch die manuellen Tests M1–M4 zu bestätigen.
        //
        // **Vollständige Fundstellenliste** (spec-reviewer-Fund: `grep -r
        // "ANNAHME A-1"` fand vorher nur diese eine Stelle), alle sind zu
        // bestätigen:
        //   1. dieser Arm und der EN-Arm darunter (`gnome-keyring`,
        //      `kwalletd6`, KeePassXC),
        //   2. die beiden `NoSessionBus`-Arme (`dbus-user-session`),
        //   3. `locales/de/common.json` und `locales/en/common.json`,
        //      Schlüssel `diagnostics.keychainUnavailable.no_session_bus`
        //      und `….no_secret_service_provider` — JSON trägt keinen
        //      Kommentar, deshalb stehen sie nur hier,
        //   4. `changelog.d/0071-linux-secret-service-meldung.md`.
        (true, KeychainUnavailableReason::NoSecretServiceProvider, Language::De) => (
            "Kein Systemschlüsselbund gefunden",
            "Smart SSH speichert Passwörter, Passphrasen und API-Keys ausschließlich im \
             Schlüsselbund des Betriebssystems. Auf diesem System läuft kein \
             Secret-Service-Anbieter.",
            "Nächster Schritt — einen Anbieter einrichten und danach neu anmelden, z. B.: \
             `sudo apt install gnome-keyring` (GNOME), `sudo apt install kwalletd6` \
             (KWallet unter KDE) oder KeePassXC mit aktivierter \
             Secret-Service-Integration.",
        ),
        (true, KeychainUnavailableReason::NoSecretServiceProvider, Language::En) => (
            "No system keyring found",
            "Smart SSH stores passwords, passphrases and API keys exclusively in the \
             operating system's keyring. No Secret Service provider is running on this \
             system.",
            "Next step — set up a provider and sign in again, for example: \
             `sudo apt install gnome-keyring` (GNOME), `sudo apt install kwalletd6` \
             (KWallet on KDE), or KeePassXC with Secret Service integration enabled.",
        ),

        // ── Kein Session-Bus (Linux) ───────────────────────────────────
        // A6: nennt ausdrücklich NICHT die Schlüsselbund-Pakete — sie
        // würden hier nichts helfen, weil ohne Session-Bus auch ein
        // installierter Anbieter nicht erreichbar ist.
        //
        // ANNAHME A-1 (Fundstelle 2, s. Liste oben): `dbus-user-session`
        // ist ebenfalls nicht gemessen.
        (true, KeychainUnavailableReason::NoSessionBus, Language::De) => (
            "Kein D-Bus-Session-Bus gefunden",
            "Smart SSH speichert Passwörter, Passphrasen und API-Keys ausschließlich im \
             Schlüsselbund des Betriebssystems. Auf diesem System ist kein \
             D-Bus-Session-Bus erreichbar — ohne ihn kann Smart SSH keinen \
             Schlüsselbund-Anbieter ansprechen, auch keinen bereits eingerichteten.",
            "Nächster Schritt — den Session-Bus bereitstellen und danach neu anmelden: \
             `sudo apt install dbus-user-session`. Ein Schlüsselbund-Paket allein hilft \
             hier nicht.",
        ),
        (true, KeychainUnavailableReason::NoSessionBus, Language::En) => (
            "No D-Bus session bus found",
            "Smart SSH stores passwords, passphrases and API keys exclusively in the \
             operating system's keyring. No D-Bus session bus is reachable on this system — \
             without it Smart SSH cannot talk to any keyring provider, not even one that is \
             already set up.",
            "Next step — provide the session bus and sign in again: \
             `sudo apt install dbus-user-session`. A keyring package on its own will not \
             help here.",
        ),

        // ── Gesperrt (Linux) ───────────────────────────────────────────
        // A7/X3: fordert zum Entsperren auf und nennt KEIN Paket. Die
        // Wörter "apt"/"install" kommen hier bewusst nirgends vor — ein
        // KDE-Nutzer, der auf diesen Rat hin `gnome-keyring` neben sein
        // laufendes KWallet setzt, hat hinterher zwei Anbieter und dasselbe
        // Problem.
        (true, KeychainUnavailableReason::Locked, Language::De) => (
            "Systemschlüsselbund gesperrt",
            "Der Systemschlüsselbund ist vorhanden, aber gesperrt — Smart SSH konnte ihn \
             nicht öffnen.",
            "Nächster Schritt — den Schlüsselbund entsperren (im \
             Schlüsselbund-Verwaltungsprogramm deiner Arbeitsumgebung oder durch eine neue \
             Anmeldung mit deinem Anmeldepasswort) und Smart SSH danach neu starten. Es \
             fehlt kein Paket: Ein zweiter Anbieter neben dem bereits laufenden würde den \
             Zustand nur verschlimmern.",
        ),
        (true, KeychainUnavailableReason::Locked, Language::En) => (
            "System keyring locked",
            "The system keyring exists but is locked — Smart SSH could not open it.",
            "Next step — unlock the keyring (in your desktop environment's keyring manager, \
             or by signing in again with your login password) and then restart Smart SSH. \
             No package is missing: a second provider next to the one already running would \
             only make things worse.",
        ),

        // ── Unbekannt, Linux ───────────────────────────────────────────
        // A4: vollwertiger Auffangfall. Bewusst ohne Paketvorschlag —
        // solange unklar ist, ob ein Anbieter fehlt oder nur gesperrt ist,
        // wäre ein `apt`-Rat genau der Fehlgriff aus X3.
        (true, KeychainUnavailableReason::Unknown, Language::De) => (
            "Systemschlüsselbund nicht verfügbar",
            "Der Systemschlüsselbund konnte nicht geöffnet werden; die genaue Ursache ließ \
             sich nicht bestimmen.",
            "Nächster Schritt — prüfen, ob ein Secret-Service-Anbieter läuft (z. B. \
             gnome-keyring, KWallet oder KeePassXC mit aktivierter \
             Secret-Service-Integration) und ob er entsperrt ist; danach Smart SSH neu \
             starten. Bewusst ohne Paketvorschlag: Solange unklar ist, ob ein Anbieter fehlt \
             oder nur gesperrt ist, kann ein zusätzliches Paket den Zustand verschlimmern.",
        ),
        (true, KeychainUnavailableReason::Unknown, Language::En) => (
            "System keyring unavailable",
            "The system keyring could not be opened; the exact cause could not be \
             determined.",
            "Next step — check whether a Secret Service provider is running (for example \
             gnome-keyring, KWallet, or KeePassXC with Secret Service integration enabled) \
             and whether it is unlocked; then restart Smart SSH. Deliberately without a \
             package suggestion: as long as it is unclear whether a provider is missing or \
             merely locked, adding one can make things worse.",
        ),

        // ── Außerhalb von Linux ────────────────────────────────────────
        // A8/T9: kein Linux-Paketname, kein `apt`, kein "Secret Service".
        (false, _, Language::De) => (
            "Systemschlüsselbund nicht verfügbar",
            "Der Systemschlüsselbund konnte nicht geöffnet werden — möglicherweise wurde \
             der Zugriff verweigert, oder der Schlüsselbund ist gesperrt.",
            "Nächster Schritt — den Systemschlüsselbund entsperren bzw. den Zugriff für \
             Smart SSH erlauben und Smart SSH danach neu starten.",
        ),
        (false, _, Language::En) => (
            "System keyring unavailable",
            "The system keyring could not be opened — access may have been denied, or the \
             keyring may be locked.",
            "Next step — unlock the system keyring or allow Smart SSH to access it, then \
             restart Smart SSH.",
        ),
    };

    DialogText {
        title: title.to_string(),
        message: format!("{state}\n\n{blocked}\n\n{next_step}\n\n{still_starts}"),
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

    const REASONS: [KeychainUnavailableReason; 4] = [
        KeychainUnavailableReason::NoSessionBus,
        KeychainUnavailableReason::NoSecretServiceProvider,
        KeychainUnavailableReason::Locked,
        KeychainUnavailableReason::Unknown,
    ];

    /// **Jeder** Startdialog-Text dieser App in einer Sprache — die Grundlage
    /// für die Tests, die über alle Fälle laufen müssen (T11b, T12, X5).
    /// Wird ein neuer Startdialog-Text ergänzt, gehört er hier hinein, sonst
    /// entgeht er diesen Prüfungen.
    fn all_startup_texts(language: Language) -> Vec<(String, DialogText)> {
        let db = Path::new("/tmp/test/smart-ssh.db");
        let logs = Path::new("/tmp/test/logs");
        let host_keys = Path::new("/tmp/test/host_keys.json");
        let mut texts = vec![
            (
                "db:schema_too_new".to_string(),
                db_connect_failure_text(
                    &ConnectFailureKind::SchemaTooNew {
                        applied_version: 15,
                        max_known_version: 12,
                    },
                    db,
                    logs,
                    language,
                ),
            ),
            (
                "db:permission_denied".to_string(),
                db_connect_failure_text(&ConnectFailureKind::PermissionDenied, db, logs, language),
            ),
            (
                "db:other".to_string(),
                db_connect_failure_text(&ConnectFailureKind::Other, db, logs, language),
            ),
            (
                "host_key_store".to_string(),
                host_key_store_failure_text(host_keys, language),
            ),
        ];
        for os in ["linux", "macos", "windows"] {
            for reason in REASONS {
                texts.push((
                    format!("keychain:{os}:{reason:?}"),
                    keychain_unavailable_text(reason, os, language),
                ));
            }
        }
        texts
    }

    #[test]
    fn test_schema_too_new_message_contains_both_version_numbers_and_the_path() {
        for (language, next_step) in [
            (Language::De, "neueste Version"),
            (Language::En, "latest version"),
        ] {
            let text = db_connect_failure_text(
                &ConnectFailureKind::SchemaTooNew {
                    applied_version: 15,
                    max_known_version: 12,
                },
                Path::new("/tmp/test/smart-ssh.db"),
                Path::new("/tmp/test/logs"),
                language,
            );
            assert!(text.message.contains("15"));
            assert!(text.message.contains("12"));
            assert!(text.message.contains("/tmp/test/smart-ssh.db"));
            assert!(
                text.message.contains(next_step),
                "{language:?} muss den nächsten Schritt (Update installieren) nennen: {}",
                text.message
            );
        }
    }

    #[test]
    fn test_permission_denied_message_mentions_the_path_and_permissions() {
        for (language, permissions) in [
            (Language::De, "zugriffsrechte"),
            (Language::En, "access permissions"),
        ] {
            let text = db_connect_failure_text(
                &ConnectFailureKind::PermissionDenied,
                Path::new("/tmp/test/smart-ssh.db"),
                Path::new("/tmp/test/logs"),
                language,
            );
            assert!(text.message.contains("/tmp/test/smart-ssh.db"));
            assert!(
                text.message.to_lowercase().contains(permissions),
                "{language:?}: {}",
                text.message
            );
        }
    }

    /// spec-reviewer-Fund: der `Other`-Auffangfall deckt auch Fälle wie
    /// SQLITE_BUSY (gesperrt durch eine zweite laufende Instanz) ab, nicht
    /// nur echte Korruption — der Text darf deshalb keine pauschale
    /// "beschädigt, Backup einspielen"-Diagnose mehr behaupten, sondern
    /// muss auf die Logdatei verweisen und Backup/Support erst als
    /// nachrangigen Schritt nennen.
    #[test]
    fn test_other_db_failure_message_is_cautious_and_points_to_the_log() {
        for (language, hedge, instance) in [
            (Language::De, "möglicherweise", "andere instanz"),
            (Language::En, "may be", "another instance"),
        ] {
            let text = db_connect_failure_text(
                &ConnectFailureKind::Other,
                Path::new("/tmp/test/smart-ssh.db"),
                Path::new("/tmp/test/logs"),
                language,
            );
            assert!(text.message.contains("/tmp/test/smart-ssh.db"));
            assert!(text.message.contains("/tmp/test/logs"));
            assert!(text.message.to_lowercase().contains("backup"));
            assert!(text.message.to_lowercase().contains("support"));
            assert!(
                text.message.to_lowercase().contains(hedge),
                "{language:?} darf Korruption nicht als sichere Diagnose behaupten: {}",
                text.message
            );
            assert!(
                text.message.to_lowercase().contains(instance)
                    || text.message.to_lowercase().contains("gesperrt")
                    || text.message.to_lowercase().contains("locked"),
                "{language:?} muss den häufigen Fall einer zweiten laufenden Instanz nennen: {}",
                text.message
            );
        }
    }

    #[test]
    fn test_host_key_store_failure_message_mentions_the_path() {
        for language in [Language::De, Language::En] {
            let text = host_key_store_failure_text(Path::new("/tmp/test/host_keys.json"), language);
            assert!(text.message.contains("/tmp/test/host_keys.json"));
        }
    }

    /// spec-reviewer-Fund: das Löschen einer beschädigten Host-Key-Datei
    /// wirft die gesamte TOFU-Pinning-Historie weg — der Text muss das
    /// nicht nur technisch erwähnen, sondern als Risiko kennzeichnen,
    /// sonst wirkt ein anschließender MITM-Server wie ein harmloser "neuer
    /// Server, bitte bestätigen".
    #[test]
    fn test_host_key_store_failure_message_warns_about_deleting_the_file() {
        let text = host_key_store_failure_text(Path::new("/tmp/test/host_keys.json"), Language::De);
        assert!(
            text.message.to_lowercase().contains("kein")
                && text.message.to_lowercase().contains("schutz"),
            "muss ausdrücklich sagen, dass Löschen KEINEN Schutz vor einem untergeschobenen \
             Server bietet: {}",
            text.message
        );
    }

    /// Spec 0071, T11c: Die englische Fassung trägt dieselbe Warnung — und
    /// zwar ebenso ausdrücklich. Ohne diesen Test könnte eine EN-Übersetzung
    /// die "KEINEN Schutz"-Aussage zu einem beiläufigen Halbsatz abschwächen
    /// und niemandem fiele es auf; das Ergebnis wäre, dass ein
    /// englischsprachiger Nutzer einen untergeschobenen Server für einen
    /// harmlosen neuen Server hält.
    #[test]
    fn test_host_key_store_failure_message_warns_about_deleting_the_file_in_english_too() {
        let text = host_key_store_failure_text(Path::new("/tmp/test/host_keys.json"), Language::En);
        let lower = text.message.to_lowercase();
        assert!(
            lower.contains("no protection"),
            "die EN-Fassung muss die 'kein Schutz'-Warnung genauso deutlich tragen: {}",
            text.message
        );
        assert!(
            lower.contains("trustworthy source") || lower.contains("server operator"),
            "die EN-Fassung muss auch den Gegenschritt nennen (Fingerabdruck extern prüfen): {}",
            text.message
        );
    }

    #[test]
    fn test_sanitize_path_for_display_replaces_control_characters() {
        for language in [Language::De, Language::En] {
            let text = host_key_store_failure_text(
                Path::new("/tmp/evil\nBitte Passwort senden"),
                language,
            );
            assert!(
                !text
                    .message
                    .lines()
                    .any(|line| line.trim() == "Bitte Passwort senden"),
                "ein Steuerzeichen im Pfad darf den Dialogtext nicht optisch fortsetzen: {}",
                text.message
            );
        }
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

    /// Spec 0071, T6: Fehlt der Anbieter, nennt der Text alle drei
    /// gängigen Anbieter und mindestens einen Installationsbefehl — das ist
    /// der Kern von BL-0031 ("klare Meldung, welches Paket fehlt").
    #[test]
    fn test_missing_provider_names_all_three_providers_and_an_install_command() {
        for language in [Language::De, Language::En] {
            let text = keychain_unavailable_text(
                KeychainUnavailableReason::NoSecretServiceProvider,
                "linux",
                language,
            );
            assert!(text.message.contains("gnome-keyring"), "{language:?}");
            assert!(text.message.contains("KWallet"), "{language:?}");
            assert!(text.message.contains("KeePassXC"), "{language:?}");
            assert!(
                text.message.contains("apt install"),
                "{language:?} muss einen konkreten Befehl nennen: {}",
                text.message
            );
        }
    }

    /// Spec 0071, T7/A6: Fehlt der Session-Bus, helfen die
    /// Schlüsselbund-Pakete nicht — der Text darf sie deshalb nicht als
    /// ersten Schritt nennen.
    #[test]
    fn test_missing_session_bus_names_the_bus_and_not_a_keyring_package() {
        for language in [Language::De, Language::En] {
            let text = keychain_unavailable_text(
                KeychainUnavailableReason::NoSessionBus,
                "linux",
                language,
            );
            assert!(
                text.message.contains("dbus-user-session"),
                "{language:?} muss das Session-Bus-Paket nennen: {}",
                text.message
            );
            assert!(
                !text.message.contains("gnome-keyring"),
                "{language:?} darf nicht auf ein Schlüsselbund-Paket zeigen: {}",
                text.message
            );
            assert!(
                !text.message.contains("KWallet") && !text.message.contains("KeePassXC"),
                "{language:?} darf nicht auf ein Schlüsselbund-Paket zeigen: {}",
                text.message
            );
        }
    }

    /// Spec 0071, T8/A7/X3: Ein gesperrter Schlüsselbund darf nie als
    /// fehlendes Paket dargestellt werden. Sonst installiert ein KDE-Nutzer
    /// `gnome-keyring` neben sein laufendes KWallet und hat hinterher zwei
    /// Anbieter und dasselbe Problem.
    #[test]
    fn test_locked_keyring_never_suggests_installing_a_package() {
        for language in [Language::De, Language::En] {
            let text =
                keychain_unavailable_text(KeychainUnavailableReason::Locked, "linux", language);
            let lower = text.message.to_lowercase();
            assert!(
                !lower.contains("apt"),
                "{language:?} darf keinen Paketmanager-Befehl enthalten: {}",
                text.message
            );
            assert!(
                !lower.contains("install"),
                "{language:?} darf nicht zum Nachinstallieren raten: {}",
                text.message
            );
            assert!(
                lower.contains("entsperr") || lower.contains("unlock"),
                "{language:?} muss zum Entsperren auffordern: {}",
                text.message
            );
        }
    }

    /// Spec 0071, T9/A8: Auf macOS und Windows gibt es weder Linux-Pakete
    /// noch einen Secret Service. Geprüft über **alle** Gründe, nicht nur
    /// über `Unknown` — der Klassifizierer liefert dort zwar nur `Unknown`,
    /// aber dieser Text darf auch bei einem künftigen Aufrufer-Fehler keinen
    /// `apt`-Rat nach macOS tragen.
    #[test]
    fn test_non_linux_texts_contain_no_linux_package_names() {
        for os in ["macos", "windows"] {
            for reason in REASONS {
                for language in [Language::De, Language::En] {
                    let text = keychain_unavailable_text(reason, os, language);
                    let lower = text.message.to_lowercase();
                    for forbidden in [
                        "gnome-keyring",
                        "kwallet",
                        "keepassxc",
                        "apt",
                        "dbus",
                        "secret service",
                        "secret-service",
                    ] {
                        assert!(
                            !lower.contains(forbidden),
                            "{os}/{reason:?}/{language:?} darf '{forbidden}' nicht enthalten: {}",
                            text.message
                        );
                    }
                }
            }
        }
    }

    /// Spec 0071, T10/A9: Der alte Satz "Alle anderen Funktionen ...
    /// funktionieren normal" war im BL-0031-Fall falsch und ist ersatzlos
    /// durch die Aufzählung der blockierten Funktionen ersetzt.
    #[test]
    fn test_every_keychain_text_lists_the_blocked_functions_and_drops_the_old_claim() {
        for os in ["linux", "macos", "windows"] {
            for reason in REASONS {
                let de = keychain_unavailable_text(reason, os, Language::De);
                assert!(
                    de.message.contains("API")
                        && de.message.contains("Passwort")
                        && de.message.contains("Passphrase"),
                    "{os}/{reason:?} muss die blockierten Funktionen aufzählen: {}",
                    de.message
                );
                assert!(
                    !de.message.contains("funktionieren normal"),
                    "{os}/{reason:?} darf den widerlegten Satz nicht mehr enthalten: {}",
                    de.message
                );

                let en = keychain_unavailable_text(reason, os, Language::En);
                let lower = en.message.to_lowercase();
                assert!(
                    lower.contains("api")
                        && lower.contains("password")
                        && lower.contains("passphrase"),
                    "{os}/{reason:?} (EN) muss die blockierten Funktionen aufzählen: {}",
                    en.message
                );
            }
        }
    }

    /// Spec 0071, T11: Die Nicht-Fatalität aus Spec 0059 bleibt — die App
    /// bricht wegen eines Schlüsselbund-Problems weiterhin nicht ab.
    #[test]
    fn test_every_keychain_text_says_the_app_still_starts() {
        for os in ["linux", "macos", "windows"] {
            for reason in REASONS {
                assert!(
                    keychain_unavailable_text(reason, os, Language::De)
                        .message
                        .contains("trotzdem gestartet"),
                    "{os}/{reason:?}"
                );
                assert!(
                    keychain_unavailable_text(reason, os, Language::En)
                        .message
                        .contains("will still start"),
                    "{os}/{reason:?}"
                );
            }
        }
    }

    /// Spec 0071, T11b: Für **jeden** Startdialog-Text gibt es beide
    /// Sprachen, und sie sind verschieden. Ohne diesen Test rutschte eine
    /// vergessene Übersetzung still als deutsche Zeichenkette durch — genau
    /// der gemischtsprachige Dialog, den A11b verhindern soll.
    #[test]
    fn test_every_startup_text_exists_in_both_languages_and_they_differ() {
        let de = all_startup_texts(Language::De);
        let en = all_startup_texts(Language::En);
        assert_eq!(de.len(), en.len());
        assert!(
            de.len() >= 16,
            "Testabdeckung unerwartet klein: {}",
            de.len()
        );
        for ((label, de_text), (_, en_text)) in de.into_iter().zip(en) {
            assert!(!de_text.message.trim().is_empty(), "{label}: DE leer");
            assert!(!en_text.message.trim().is_empty(), "{label}: EN leer");
            assert_ne!(
                de_text.message, en_text.message,
                "{label}: EN-Fassung fehlt (identisch mit DE)"
            );
            assert_ne!(
                de_text.title, en_text.title,
                "{label}: EN-Titel fehlt (identisch mit DE)"
            );
        }
    }

    /// Spec 0071, X5: Kein Text darf zu einem Klartext-Ablageort oder einem
    /// passwortlosen Schlüsselbund raten. Smart SSH legt Secrets
    /// ausschließlich im OS-Schlüsselbund ab (I2, §2 Nicht-Ziel 1) — ein
    /// Meldungstext ist nicht der Ort, an dem diese Zusage aufgeweicht wird.
    #[test]
    fn test_no_startup_text_recommends_a_less_secure_place_for_secrets() {
        for language in [Language::De, Language::En] {
            for (label, text) in all_startup_texts(language) {
                let lower = text.message.to_lowercase();
                for forbidden in [
                    "klartext",
                    "plain text",
                    "plaintext",
                    "ohne passwort",
                    "empty password",
                    "leeres passwort",
                    "umgebungsvariable",
                    "environment variable",
                ] {
                    assert!(
                        !lower.contains(forbidden),
                        "{label}/{language:?} enthält den unsicheren Rat '{forbidden}': {}",
                        text.message
                    );
                }
            }
        }
    }

    /// Spec 0059, Invarianten: "kein Secret/kein sensibler Inhalt im
    /// Dialog-Text". Keine dieser Funktionen nimmt überhaupt einen rohen
    /// Fehler/DB-Inhalt als Parameter entgegen (nur `ConnectFailureKind`,
    /// `Path`, `&str`) — dieser Test dokumentiert die Invariante trotzdem
    /// exekutierbar: keiner der erzeugten Texte enthält versehentlich
    /// einen Platzhalter-"Secret"-artigen String.
    ///
    /// Spec 0071, T12: auf **alle** neuen Texte ausgeweitet (beide Sprachen,
    /// alle vier Gründe, alle drei Ziel-Betriebssysteme) über
    /// [`all_startup_texts`].
    #[test]
    fn test_no_generated_text_leaks_a_placeholder_secret_value() {
        for language in [Language::De, Language::En] {
            for (label, text) in all_startup_texts(language) {
                assert!(!text.message.contains("hunter2"), "{label}");
                assert!(!text.message.contains("sk-live"), "{label}");
                assert!(
                    !text.message.to_lowercase().contains("password="),
                    "{label}"
                );
            }
        }
    }

    /// Spec 0071, X2 (zweite Hälfte) und I1: Der Schlüsselbund-Text hängt
    /// ausschließlich an `(Grund, Ziel-OS, Sprache)` — es gibt keinen
    /// Parameter, über den ein roher Bibliotheksfehler oder eine D-Bus-
    /// Adresse hineinkommen könnte (A10). Exekutierbar festgehalten:
    /// derselbe Aufruf liefert immer denselben Text, unabhängig von allem
    /// anderen im Prozess.
    #[test]
    fn test_keychain_text_depends_on_nothing_but_reason_os_and_language() {
        for os in ["linux", "macos", "windows"] {
            for reason in REASONS {
                for language in [Language::De, Language::En] {
                    let first = keychain_unavailable_text(reason, os, language);
                    let second = keychain_unavailable_text(reason, os, language);
                    assert_eq!(first.message, second.message);
                    assert_eq!(first.title, second.title);
                    assert!(
                        !first.message.contains("unix:path="),
                        "eine D-Bus-Adresse hat in keinem Text etwas zu suchen: {}",
                        first.message
                    );
                }
            }
        }
    }
}
