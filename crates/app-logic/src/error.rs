//! The error type of Tauri commands and the conversions into it.
//!
//! [`CommandError`] (defined in `ssh_manager_core::ipc_error`) always carries
//! a stable `code` (Spec 0024, section 5). There is no blanket
//! `From<E: Display>`: an error type reaches `?` only if it implements
//! [`ErrorCode`] (or has an explicit `From`, like `FeatureLocked` and
//! `RuleWriteError`). Credential-store errors are deliberately **not**
//! `ErrorCode` — they go through [`secret_store_error`] so the backend
//! payload never reaches the message. Plain strings are not convertible: use
//! `CommandError::with_code(message, CODE)`.

use ssh_manager_core::profiles::CredentialError;
use ssh_manager_core::ssh::SshError;

/// Spec 0071, A13: stabiler Code für "der OS-Schlüsselbund ist bei diesem
/// Programmlauf nicht erreichbar". Das Frontend übersetzt ihn über
/// `errorCodes.ts` und zeigt den eigenen Text, statt den englischen
/// `Display`-Text der `keyring`-Crate durchzureichen.
pub const KEYCHAIN_UNAVAILABLE: &str = "KEYCHAIN_UNAVAILABLE";

/// Spec 0098, A1: „der Schlüsselbund war beim Start da, dieser Zugriff ist
/// trotzdem gescheitert". Die Konstante wohnt in `core`
/// ([`ssh_manager_core::profiles::KEYCHAIN_ACCESS_FAILED`]), weil
/// [`ssh_manager_core::ssh::SshError::code`] sie für die Verbindungskette
/// (A4) ebenfalls vergibt — ein zweites Literal hier könnte abdriften.
pub use ssh_manager_core::profiles::KEYCHAIN_ACCESS_FAILED;

/// Spec 0101, A9.1: „der Secret-Speicher hat den Zugriff nicht ausgeführt".
/// Wohnt aus demselben Grund in `core` wie [`KEYCHAIN_ACCESS_FAILED`] —
/// `SshError::code` vergibt ihn für die Verbindungskette ebenfalls.
pub use ssh_manager_core::profiles::SECRET_STORE_FAILED;

/// A credential reference without a stored entry (a statement about the
/// stock, not a store failure).
pub const CREDENTIAL_NOT_FOUND: &str = "CREDENTIAL_NOT_FOUND";

/// Spec 0098 A1/A2/A7, seit Spec 0101 A9.1 auf den Secret-Speicher
/// umgestellt: der eine Ort, an dem ein [`CredentialError::Backend`] zu
/// Code und Meldung wird.
///
/// **Die Nutzlast bleibt hier liegen** (A5, Ausweitung von Spec 0071 X2).
/// Sie geht nur noch in eine `debug`-Zeile (A7) — und die läuft durch den
/// Redactor. `debug` ist ohne `RUST_LOG` aus (Spec 0094, A3).
///
/// **Die Code-Wahl hängt an der Variante, nie am Text** (A1,
/// Angriffsrichtung T10): Eine Nutzlast, die selbst wie ein Code aussieht,
/// fließt nicht in diese Entscheidung ein — sie wird gar nicht gelesen.
/// Seit A9.1 gibt es überhaupt nur noch **einen** Code auf diesem Weg; der
/// Startzustand des Schlüsselbunds spielt keine Rolle mehr.
fn backend_failure_to_command_error(payload: &str) -> CommandError {
    // Durch den Redactor, wie die Zeile in `core`s `credential_lookup_error`
    // — Begründung dort (spec-reviewer Runde 1): A5 erklärt diese Nutzlast
    // für unzustellbar, und ein Log ist eine Datensenke.
    tracing::debug!(
        error = %{
            use ssh_manager_core::ai::OutputRedactor;
            ssh_manager_core::ai::default_log_redactor().redact_text(payload)
        },
        "secret store backend failure (Spec 0101, A9.1)"
    );
    // Kein „Schlüsselbund" im Text (A9.1). Den ausführlichen Text liefert
    // das Frontend über den Code; diese Meldung ist der Fallback für einen
    // Aufrufer, der ihn nicht kennt.
    CommandError::with_code(
        "Der Zugriff auf den Secret-Speicher ist fehlgeschlagen. \
         Die gespeicherten Zugangsdaten konnten nicht gelesen oder \
         geschrieben werden.",
        SECRET_STORE_FAILED,
    )
}

/// Spec 0076, C-6: Die Überführung einer Schlüsseldatei in den
/// Schlüsselbund ist gescheitert, **und** der eben geschriebene Schlüssel
/// ließ sich nicht wieder entfernen — es liegt jetzt ein privater Schlüssel
/// im Schlüsselbund, auf den kein Server zeigt.
///
/// **Eigener Code, nicht [`KEYCHAIN_UNAVAILABLE`]** (spec-reviewer-Fund,
/// Runde 2): Das Frontend **ersetzt** bei einem bekannten Code die Meldung
/// durch seinen eigenen Text (`errorCodes.ts`). Mit
/// `KEYCHAIN_UNAVAILABLE` hätte der Nutzer „Der Systemschlüsselbund ist
/// nicht verfügbar" gelesen — und ausgerechnet **nicht** erfahren, welcher
/// Eintrag liegen geblieben ist. Das ist die Auskunft, für die dieser Weg
/// überhaupt existiert.
///
/// Kennt das Frontend den Code nicht, zeigt es die `message`, die den Slot
/// nennt. Eine Übersetzung sollte ihn als Parameter führen.
pub const IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND: &str = "IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND";

/// Spec 0077, 3.1.3: Das Muster einer Filterregel lässt sich nicht
/// übersetzen — die Regel wurde deshalb **nicht** gespeichert.
pub use persistence_sqlite::FILTER_RULE_PATTERN_INVALID;

/// Issue #357, Spec 0086 A1.1: „Dateiinhalt kopieren" lehnt eine Datei über
/// 50 MB ab. Das Frontend übersetzt den Code (`errorCodes.ts`), der Text hier
/// ist nur der Fallback.
pub const SFTP_CONTENT_TOO_LARGE: &str = "SFTP_CONTENT_TOO_LARGE";

/// Issue #655: the text preview (`sftp_read_text`) refused a file for its
/// size / because it is not valid UTF-8. The frontend names the reason in the
/// "no diff" hint; the message stays the fallback.
pub const SFTP_PREVIEW_TOO_LARGE: &str = "SFTP_PREVIEW_TOO_LARGE";
pub const SFTP_PREVIEW_NOT_TEXT: &str = "SFTP_PREVIEW_NOT_TEXT";

/// Issue #357, Spec 0086 A1.5: der Nutzer hat „Dateiinhalt kopieren"
/// abgebrochen. Kein Fehler im eigentlichen Sinn; das Frontend zeigt dazu
/// nichts an.
pub const SFTP_COPY_CANCELLED: &str = "SFTP_COPY_CANCELLED";

/// Issue #358: „Datei kopieren" konnte die Datei nicht als Dateiverweis in
/// die Zwischenablage legen (Plattform oder Sitzung ohne Unterstützung).
pub const CLIPBOARD_FILE_FAILED: &str = "CLIPBOARD_FILE_FAILED";

/// Issue #856: the risk second-opinion setting named a provider that is not a
/// configured AI provider.
pub const RISK_SETTINGS_PROVIDER_UNKNOWN: &str = "RISK_SETTINGS_PROVIDER_UNKNOWN";

/// Issue #856: the risk second opinion was switched on without a provider.
pub const RISK_SETTINGS_PROVIDER_REQUIRED: &str = "RISK_SETTINGS_PROVIDER_REQUIRED";

/// Issue #862: `set_ui_preference` refused a key that is not a registered UI
/// preference, or a value outside the shape registered for it.
pub const UI_PREFERENCE_REJECTED: &str = "UI_PREFERENCE_REJECTED";

pub use ssh_manager_core::profiles::{
    STORE_BUSY, STORE_CONSTRAINT, STORE_CORRUPT, STORE_FAILED, STORE_IO,
};

/// `result.store_err()?` — kept as a readable marker at store call sites;
/// the code comes from the error's [`ErrorCode`] impl.
pub trait StoreResultExt<T> {
    fn store_err(self) -> CommandResult<T>;
}

impl<T, E: ErrorCode + std::fmt::Display> StoreResultExt<T> for Result<T, E> {
    fn store_err(self) -> CommandResult<T> {
        self.map_err(CommandError::from)
    }
}

/// `result.or_code("SOME_CODE")?` — gives an error that has no [`ErrorCode`]
/// (a plain `String`/`&str`, a foreign-crate error) an explicit code and keeps
/// its `Display` text as the message. The code is chosen at the call site, so
/// a conversion that would drop it does not exist.
pub trait ResultCodeExt<T> {
    fn or_code(self, code: &'static str) -> CommandResult<T>;
}

impl<T, E: std::fmt::Display> ResultCodeExt<T> for Result<T, E> {
    fn or_code(self, code: &'static str) -> CommandResult<T> {
        self.map_err(|err| CommandError::with_code(err.to_string(), code))
    }
}

/// Issue #883: a command named a session that is no longer open.
pub const SESSION_NOT_FOUND: &str = "SESSION_NOT_FOUND";

/// Spec 0077, 3.1.3: Wandelt einen [`RuleWriteError`] in einen
/// [`CommandError`] und hängt für ein ungültiges Muster den Code
/// [`FILTER_RULE_PATTERN_INVALID`] an.
///
/// **Ein explizites `From` statt `ErrorCode`** (Issue #260, ADR 0068 §7):
/// `RuleWriteError` hat bewusst kein `Display`; ein bloßes `?` behält den
/// Code.
///
/// Als `message` steht der Fehlertext der Bibliothek (mit der Stelle im
/// Muster). Das Frontend ersetzt bei bekanntem Code zwar den Text, zeigt
/// die `message` für diesen Code aber zusätzlich darunter an (3.1.4) —
/// genau dafür wird sie hier mitgegeben.
impl From<crate::filter_rules::RuleWriteError> for CommandError {
    fn from(err: crate::filter_rules::RuleWriteError) -> Self {
        match err {
            crate::filter_rules::RuleWriteError::InvalidPattern(pattern_err) => {
                CommandError::with_code(pattern_err.to_string(), FILTER_RULE_PATTERN_INVALID)
            }
            crate::filter_rules::RuleWriteError::Store(
                persistence_sqlite::PolicyStoreError::InvalidPattern(pattern_err),
            ) => CommandError::with_code(pattern_err.to_string(), FILTER_RULE_PATTERN_INVALID),
            crate::filter_rules::RuleWriteError::Store(store_err) => CommandError::from(store_err),
        }
    }
}

/// Spec 0101, A9.1 (zuvor Spec 0071 A13/X2 und Spec 0098 A1/A2): Wandelt
/// einen [`CredentialError`] des **Secret-Speichers** in einen
/// [`CommandError`] und hängt jedem [`CredentialError::Backend`] den
/// stabilen Code [`SECRET_STORE_FAILED`] an.
///
/// **Warum der Schlüsselbund hier nicht mehr vorkommt:** Seit A9 liegen die
/// Secrets in der verschlüsselten Datenbank
/// ([`persistence_sqlite::SqliteCredentialStore`]); der Schlüsselbund trägt
/// nur noch K. Jeder Aufrufer dieser Funktion arbeitet auf
/// `AppState.credential_store`, also auf der Datenbank. Hinge der Code
/// weiterhin an `AppState.keychain`, meldete die App für einen
/// Datenbankfehler „Schlüsselbund gesperrt" und schickte den Nutzer in die
/// Systemeinstellungen, wo es nichts zu reparieren gibt. A9.1 verlangt
/// deshalb ausdrücklich: eigener Code, kein `KEYCHAIN_*`, kein
/// „Schlüsselbund" im Text, **und keine Abhängigkeit von
/// `AppState.keychain`** — der Parameter ist mit dieser Spec entfallen.
///
/// **Der Code ersetzt den Text, er ergänzt ihn nicht** (unverändert aus
/// Spec 0098 A5/X2): Die Nutzlast von [`CredentialError::Backend`] erreicht
/// das Frontend auf keinem Weg, auch nicht im Feld `message`. Enthielte ein
/// Backend-Fehler jemals einen Secret-artigen Wert, käme er über diesen Pfad
/// nicht ins Frontend. Zur Diagnose bleibt er auf `debug`
/// (A7, s. [`backend_failure_to_command_error`]).
///
/// [`CredentialError::NotFound`] bleibt bewusst unverändert: „kein Eintrag
/// vorhanden" ist eine fachliche Aussage über den Bestand, keine Störung des
/// Speichers — sie mit einem Störungscode zu überschreiben wäre genau die
/// Verwechslung, die Spec 0071 A14/I4 verbietet.
pub fn secret_store_error(err: CredentialError) -> CommandError {
    match err {
        CredentialError::Backend(payload) => backend_failure_to_command_error(&payload),
        // Explicitly `NotFound`, no catch-all: a third `CredentialError`
        // variant must fail to compile here rather than leak raw text. The
        // message is the `Display` text (names only the reference).
        not_found @ CredentialError::NotFound(_) => {
            CommandError::with_code(not_found.to_string(), CREDENTIAL_NOT_FOUND)
        }
    }
}

/// Spec 0098 A4/A5, seit Spec 0101 A9.1 ohne Schlüsselbund-Bezug: ein
/// [`SshError`] als [`CommandError`].
///
/// **Die Umdeutung nach `KEYCHAIN_UNAVAILABLE` ist ersatzlos entfallen.**
/// Sie war die eine Stelle, an der der Startzustand des Schlüsselbunds den
/// Code eines Verbindungsfehlers verändern konnte. Mit A9 liegt hinter
/// [`SshError::CredentialStoreFailed`] die Datenbank, nicht der
/// Schlüsselbund — die Umdeutung hätte also ab jetzt systematisch die
/// falsche Ursache benannt. [`SshError::code`] vergibt
/// [`SECRET_STORE_FAILED`] endgültig; keine Schicht darüber ändert ihn noch.
///
/// Die `message` bleibt `SshError`s `Display`. Für
/// [`SshError::CredentialStoreFailed`] ist das per Konstruktion fester Text
/// plus Secret-Art plus Hop-Angabe (Spec 0076, A-8) — die Nutzlast der
/// Bibliothek hat dort kein Feld (A5).
pub fn ssh_command_error(err: &SshError) -> CommandError {
    CommandError::with_code(err.to_string(), err.code())
}

/// The command error type lives in `ssh_manager_core::ipc_error` so that
/// extension crates without an `app-logic` dependency can return it too.
pub use ssh_manager_core::ipc_error::{CommandError, CommandResult, ErrorCode};

#[cfg(test)]
mod code_tests {
    /// Spec 0024, Abschnitt 5: Codes müssen stabil und eindeutig sein — kein
    /// Code darf für zwei unterschiedliche Validierungsfehler doppelt
    /// vergeben sein. Enumeriert alle aktuell über `CommandError::with_code`
    /// vergebenen Codes (Server-/Gruppen-Formulare, Spec 0008); s.
    /// `groups.rs`/`server_credentials.rs` für die jeweiligen
    /// Einzeltests, die zusätzlich prüfen, dass der *richtige* Code am
    /// jeweiligen Fehlerfall hängt.
    #[test]
    fn test_command_error_with_code_values_are_unique() {
        let codes = [
            "GROUP_SELF_PARENT",
            "GROUP_CYCLE_DETECTED",
            "SERVER_PASSWORD_REQUIRED",
            "SERVER_PRIVATE_KEY_REQUIRED",
            "SERVER_CERTIFICATE_REQUIRED",
            "SERVER_CERTIFICATE_KEY_REQUIRED",
            "FIRST_RUN_NOTICE_NOT_ACKNOWLEDGED",
            "SERVER_JUMP_HOST_LOCAL",
            // Spec 0069, Teil A4:
            "AI_NO_ACTIVE_PROVIDER",
            "SSH_HOST_KEY_NOT_TRUSTED",
            "SSH_HOST_KEY_CONFIRM_TIMEOUT",
            // Spec 0069, Teil A5 (spec-reviewer-Fund):
            "SSH_CONNECTION_ABANDONED",
            // Issue #546: the user cancelled a running connect.
            "SSH_CONNECT_CANCELLED",
            // Spec 0071, A13.
            super::KEYCHAIN_UNAVAILABLE,
            // Spec 0098, A1/A6 (BL-0244/BL-0205/BL-0206).
            super::KEYCHAIN_ACCESS_FAILED,
            // Spec 0101, A9.1 (BL-0314). Kommt sowohl aus
            // `secret_store_error` als auch aus
            // `ssh_manager_core::ssh::SshError::code` — derselbe Wert,
            // deshalb nur ein Eintrag.
            super::SECRET_STORE_FAILED,
            // Spec 0076 (BL-0221/BL-0222). Die `KEY_FILE_*`-Codes kommen
            // aus `ssh_manager_core::ssh::KeyFileError::code()` und werden
            // über `identity_file::identity_file_error_to_command_error`
            // in einen `CommandError` gehängt — sie gehören deshalb in
            // dieselbe Eindeutigkeitsprüfung wie die hier direkt
            // vergebenen.
            "SERVER_IDENTITY_FILE_REQUIRED",
            "SERVER_NOT_AN_IDENTITY_FILE",
            super::IDENTITY_FILE_ROLLBACK_LEFT_KEY_BEHIND,
            "KEY_FILE_NOT_FOUND",
            "KEY_FILE_NOT_FOUND_ON_DEVICE",
            "KEY_FILE_NOT_READABLE",
            "KEY_FILE_PERMISSIONS_TOO_OPEN",
            "KEY_FILE_TOO_LARGE",
            "KEY_FILE_NOT_A_REGULAR_FILE",
            "KEY_FILE_PATH_NOT_ABSOLUTE",
            "KEY_FILE_INVALID_KEY",
            // Spec 0077, 3.1.3 (BL-0249).
            super::FILTER_RULE_PATTERN_INVALID,
            // Issue #357, Spec 0086 A1.1/A1.5.
            super::SFTP_CONTENT_TOO_LARGE,
            super::SFTP_COPY_CANCELLED,
            // Issue #655.
            super::SFTP_PREVIEW_TOO_LARGE,
            super::SFTP_PREVIEW_NOT_TEXT,
            // Issue #358.
            super::CLIPBOARD_FILE_FAILED,
            // Issue #856.
            super::RISK_SETTINGS_PROVIDER_UNKNOWN,
            super::RISK_SETTINGS_PROVIDER_REQUIRED,
            // Issue #862.
            super::UI_PREFERENCE_REJECTED,
            // Issue #885.
            super::STORE_BUSY,
            super::STORE_CONSTRAINT,
            super::STORE_CORRUPT,
            super::STORE_IO,
            super::STORE_FAILED,
            // Issue #883.
            super::SESSION_NOT_FOUND,
        ];
        assert_unique(&codes);
        // The codes of every `ErrorCode` type come from the types' own lists,
        // so a new variant cannot be forgotten here. A code the list above
        // names too (e.g. `SECRET_STORE_FAILED`) is counted once.
        let mut typed: Vec<&str> = Vec::new();
        for list in [
            ssh_manager_core::ssh::SshError::CODES,
            ssh_manager_core::ai::AiError::CODES,
            ssh_manager_core::profiles::ProfileError::CODES,
            persistence_sqlite::STORE_ERROR_CODES,
            &[ssh_manager_core::ipc_error::IO_FAILED],
        ] {
            assert_unique(list);
            typed.extend(list.iter().copied().filter(|c| !codes.contains(c)));
        }
        assert_unique(&typed);
        assert!(typed.iter().all(|c| !codes.contains(c)));
    }

    /// Fails when a code appears twice in `codes`.
    fn assert_unique(codes: &[&str]) {
        let mut unique = codes.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            codes.len(),
            unique.len(),
            "doppelt vergebener CommandError-Code: {codes:?}"
        );
    }

    #[test]
    #[should_panic(expected = "doppelt vergebener")]
    fn test_assert_unique_rejects_a_duplicate() {
        assert_unique(&["A", "B", "A"]);
    }

    /// Spec 0077, T-6d: Die Umwandlung aus 3.1.3 hängt für ein ungültiges
    /// Muster den stabilen Code an und reicht den Fehlertext der
    /// Bibliothek als `message` durch.
    ///
    /// Scheitert, wenn `RuleWriteError` wieder `Display` bekäme (E0119) oder
    /// die Umwandlung den Code nicht setzt.
    #[test]
    fn test_spec_0077_t6d_invalid_pattern_keeps_its_code_and_library_message() {
        let pattern_err =
            ssh_manager_core::filter::Pattern::Regex("^systemctl stop (.*".to_string())
                .validate()
                .unwrap_err();
        let expected_message = pattern_err.to_string();

        let err = crate::error::CommandError::from(
            crate::filter_rules::RuleWriteError::InvalidPattern(pattern_err),
        );

        assert_eq!(err.code, Some(super::FILTER_RULE_PATTERN_INVALID));
        assert_eq!(err.message, expected_message);
        assert!(
            err.message.contains("unclosed group"),
            "die Stelle im Muster gehört in die Meldung: {}",
            err.message
        );
    }

    /// Issue #260: Ein bloßes `?` über einen `RuleWriteError` liefert einen
    /// `CommandError` mit Code — auch wenn das Muster erst im Speicher
    /// (Schicht 2) abgewiesen wurde.
    ///
    /// *Gegenbeweis:* Vor #260 lief `?` über den blanket `From<E: Display>`
    /// und ergab `code: None`.
    #[test]
    fn test_issue_260_question_mark_keeps_the_code() {
        fn store_layer_rejects() -> Result<(), crate::filter_rules::RuleWriteError> {
            let pattern_err = ssh_manager_core::filter::Pattern::Regex("^a(".to_string())
                .validate()
                .unwrap_err();
            Err(persistence_sqlite::PolicyStoreError::InvalidPattern(
                pattern_err,
            ))?
        }
        fn command() -> crate::error::CommandResult<()> {
            store_layer_rejects()?;
            Ok(())
        }
        let err = command().unwrap_err();
        assert_eq!(err.code, Some(super::FILTER_RULE_PATTERN_INVALID));
    }

    /// Spec 0077, 3.1.3: Ein Speicher-Fehler wird umgewandelt wie bisher —
    /// ohne Code. Belegt, dass die neue Umwandlung nicht pauschal den
    /// Muster-Code an jeden Schreibfehler hängt.
    #[test]
    fn test_spec_0077_store_error_keeps_converting_without_a_code() {
        let err = crate::error::CommandError::from(crate::filter_rules::RuleWriteError::Store(
            persistence_sqlite::PolicyStoreError::NotFound(ssh_manager_core::filter::RuleId(
                "irgendeine-regel".to_string(),
            )),
        ));

        // The kind survives as a code; a missing rule is not a store failure.
        assert!(err.code.is_some());
        assert_ne!(err.code, Some(ssh_manager_core::profiles::STORE_FAILED));
    }
}

#[cfg(test)]
mod secret_store_code_tests {
    use super::*;
    use ssh_manager_core::profiles::CredentialRef;

    /// Spec 0101, T10 / Spec 0071 A13: Ein Störungsfehler des
    /// Secret-Speichers erreicht das Frontend mit einem stabilen Code —
    /// nicht mehr als codeloser, englischer Bibliothekstext.
    ///
    /// **Gegen den Stand vor Spec 0101 scheitert das:** dort war der Code
    /// `KEYCHAIN_ACCESS_FAILED` bzw. `KEYCHAIN_UNAVAILABLE`, je nach
    /// Startzustand des Schlüsselbunds.
    #[test]
    fn test_backend_error_gets_the_stable_secret_store_code() {
        let err = secret_store_error(CredentialError::Backend("database is locked".to_string()));
        assert_eq!(err.code, Some(SECRET_STORE_FAILED));
    }

    /// Spec 0071, X2: Der Code **ersetzt** den Text. Ein Platzhalter-Secret
    /// aus der Fehlerkette darf in keiner `CommandError.message` mit diesem
    /// Code auftauchen — sonst hätte die Redaktionszusage genau hier ein
    /// Loch, an einer Stelle, die direkt ins UI geht.
    #[test]
    fn test_backend_error_text_never_reaches_the_frontend_with_this_code() {
        let err = secret_store_error(CredentialError::Backend(
            "token sk-live-hunter2 rejected by the store".to_string(),
        ));
        assert_eq!(err.code, Some(SECRET_STORE_FAILED));
        assert!(
            !err.message.contains("hunter2") && !err.message.contains("sk-live"),
            "der rohe Backend-Fehlertext darf nicht mitgereicht werden: {}",
            err.message
        );
    }

    /// Die Nutzlast, die laut Spec §7 in jedem Test dieses Laufs steht.
    /// Enthält absichtlich **beide** Teile — den „Bibliothekstext" und ein
    /// „Geheimnis" —, damit ein Test, der nur einen davon sucht, nicht aus
    /// Versehen grün ist.
    const MARKER: &str = "LIBTEXT-0098 Geheim-0098";

    /// Spec 0098, T1/T2 (A1), seit Spec 0101 A9.1 mit dem neuen Code:
    /// Ein Schreib- oder Lesefehler des Secret-Speichers reicht den rohen
    /// Bibliothekstext **nicht** ins Frontend durch, sondern bekommt einen
    /// stabilen Code.
    #[test]
    fn test_spec_0101_a9_1_backend_error_gets_the_secret_store_code() {
        let err = secret_store_error(CredentialError::Backend(MARKER.to_string()));

        assert_eq!(err.code, Some(SECRET_STORE_FAILED));
        assert!(
            !err.message.contains("LIBTEXT-0098") && !err.message.contains("Geheim-0098"),
            "die Nutzlast der Bibliothek darf das Frontend nicht erreichen: {}",
            err.message
        );
    }

    /// Spec 0101, A9.1 / T10: Die Meldung nennt den **Schlüsselbund
    /// nicht**. Seit A9 liegen die Secrets in der Datenbank; „Schlüsselbund
    /// gesperrt" schickte den Nutzer in die Systemeinstellungen, wo es
    /// nichts zu reparieren gibt. Ebensowenig behauptet sie „nicht
    /// verfügbar" oder rät zu einer Paketinstallation.
    ///
    /// **Gegen den Stand vor Spec 0101 scheitert das:** dort lautete die
    /// Meldung wörtlich „Der Zugriff auf den Systemschlüsselbund ist
    /// fehlgeschlagen …".
    #[test]
    fn test_spec_0101_a9_1_message_never_mentions_the_os_keychain() {
        let err = secret_store_error(CredentialError::Backend(MARKER.to_string()));

        let message = err.message.to_lowercase();
        assert!(
            !message.contains("schlüsselbund") && !message.contains("keychain"),
            "A9.1: kein „Schlüsselbund\" im Text: {}",
            err.message
        );
        assert!(
            !message.contains("nicht verfügbar"),
            "„nicht verfügbar\" ist genau die falsche Auskunft: {}",
            err.message
        );
        assert!(
            !message.contains("apt ") && !message.contains("install"),
            "kein Paket- oder Installationshinweis: {}",
            err.message
        );
    }

    /// Spec 0101, A9.1: Es gibt **keine** zweite Fassung dieses Codes mehr.
    /// Vor dieser Spec entschied der Startzustand des Schlüsselbunds
    /// zwischen `KEYCHAIN_ACCESS_FAILED` und `KEYCHAIN_UNAVAILABLE`; jetzt
    /// gibt es nur noch einen Ausgang, und zwar immer denselben.
    ///
    /// Dass der Startzustand nicht mehr einfließen **kann**, hängt an der
    /// Signatur: [`secret_store_error`] bekommt ihn gar nicht mehr. Der
    /// Test hält fest, was daraus folgt — ein Fehler, beliebig oft, immer
    /// derselbe Code, nie ein `KEYCHAIN_*`.
    #[test]
    fn test_spec_0101_a9_1_there_is_no_second_code_for_this_path() {
        for attempt in 1..=3 {
            let err = secret_store_error(CredentialError::Backend(MARKER.to_string()));

            assert_eq!(
                err.code,
                Some(SECRET_STORE_FAILED),
                "Versuch {attempt}: immer derselbe Code"
            );
            assert_ne!(err.code, Some(KEYCHAIN_UNAVAILABLE));
            assert_ne!(err.code, Some(KEYCHAIN_ACCESS_FAILED));
        }
    }

    /// Spec 0098, T10 (adversarial): Die Code-Wahl hängt an der
    /// **Variante**, nie am Text der Nutzlast. Eine Nutzlast, die selbst
    /// wie ein anderer Code aussieht, ändert daran nichts — und landet
    /// insbesondere nicht in der `message`, wo sie wie eine Aussage der App
    /// über ihren eigenen Zustand gelesen würde.
    #[test]
    fn test_spec_0098_t10_payload_that_looks_like_a_code_does_not_pick_the_code() {
        let err = secret_store_error(CredentialError::Backend(KEYCHAIN_UNAVAILABLE.to_string()));

        assert_eq!(
            err.code,
            Some(SECRET_STORE_FAILED),
            "die Variante entscheidet, nicht die Nutzlast"
        );
        assert!(
            !err.message.contains(KEYCHAIN_UNAVAILABLE),
            "die Nutzlast darf auch dann nicht durchkommen, wenn sie wie ein Code aussieht: {}",
            err.message
        );
    }

    /// Spec 0098, T9 (adversarial): Eine Nutzlast mit Zeilenumbruch und
    /// Anführungszeichen — die Form, in der ein Bibliothekstext ein
    /// mitgeliefertes Geheimnis wie ein eigenes Feld aussehen lässt — kommt
    /// in **keinem** Feld ans Frontend.
    #[test]
    fn test_spec_0098_t9_multiline_payload_reaches_no_field() {
        let payload = "x\nPasswort: Geheim-0098\"";

        let err = secret_store_error(CredentialError::Backend(payload.to_string()));

        // Nicht nur `message`: Das ganze serialisierte `CommandError`,
        // also exakt das, was über die IPC-Grenze geht.
        let serialised = serde_json::to_string(&err).expect("CommandError ist serialisierbar");
        assert!(
            !serialised.contains("Geheim-0098"),
            "die Nutzlast darf in keinem Feld des DTO stehen: {serialised}"
        );
    }

    /// Spec 0071, A14/I4: „nicht vorhanden" ist keine Störung des
    /// Speichers und bekommt deshalb **keinen** Störungscode.
    #[test]
    fn test_not_found_is_never_reported_as_a_store_failure() {
        let err = secret_store_error(CredentialError::NotFound(CredentialRef::new(
            "server:1:sudo_password".to_string(),
        )));
        assert_eq!(err.code, Some(CREDENTIAL_NOT_FOUND));
        assert_ne!(err.code, Some(SECRET_STORE_FAILED));
    }

    /// Spec 0101, A9.1, zweiter Weg (§1): derselbe Code aus der
    /// Verbindungskette. [`ssh_command_error`] deutet ihn **nicht** mehr
    /// um — vor dieser Spec wurde daraus bei nicht verfügbarem
    /// Schlüsselbund `KEYCHAIN_UNAVAILABLE`.
    #[test]
    fn test_spec_0101_a9_1_the_connection_path_reports_the_same_code() {
        use ssh_manager_core::ssh::SecretKind;

        let err = ssh_command_error(&SshError::CredentialStoreFailed {
            secret: SecretKind::Password,
            hop: None,
        });

        assert_eq!(err.code, Some(SECRET_STORE_FAILED));
        let message = err.message.to_lowercase();
        assert!(
            !message.contains("schlüsselbund") && !message.contains("keychain"),
            "A9.1: auch auf diesem Weg kein „Schlüsselbund\" im Text: {}",
            err.message
        );
    }

    /// Spec 0098, A4 letzter Satz / Spec 0071 I4: Ein fehlender Eintrag
    /// bleibt auf dem Verbindungsweg `SSH_CREDENTIAL_RESOLUTION_FAILED` —
    /// die Umstellung aus A9.1 färbt nicht auf andere Varianten ab.
    #[test]
    fn test_spec_0101_a9_1_other_ssh_errors_keep_their_code() {
        let err = ssh_command_error(&SshError::CredentialResolutionFailed(
            "Passwort: kein Credential".to_string(),
        ));
        assert_eq!(err.code, Some("SSH_CREDENTIAL_RESOLUTION_FAILED"));
    }
}

#[cfg(test)]
mod feature_locked_tests {
    use super::*;
    use ssh_manager_core::entitlements::{Feature, TierId};

    /// Spec 0037, Abschnitt 2: `FeatureLocked` muss strukturiert (nicht nur
    /// als generischer `message`-String über den blanket `From<E: Display>`)
    /// im `CommandError` landen, damit das Frontend ihn eindeutig von
    /// fachlichen Fehlern unterscheiden kann.
    #[test]
    fn test_command_error_feature_locked_carries_structured_feature_and_tier() {
        let err = ssh_manager_core::entitlements::FeatureLocked {
            feature: Feature::new("document_export"),
            tier: TierId::FREE,
        };

        let command_error = CommandError::feature_locked(err);

        assert_eq!(command_error.code, Some("FEATURE_LOCKED"));
        let feature_locked = command_error
            .feature_locked
            .expect("feature_locked muss gesetzt sein");
        assert_eq!(feature_locked.feature, Feature::new("document_export"));
        assert_eq!(feature_locked.tier, TierId::FREE);
    }

    /// Gegentest: ein gewöhnlicher Fehler (über `with_code`) darf `feature_locked` nicht setzen — sonst könnte das
    /// Frontend jeden Fehler fälschlich als gesperrtes Feature behandeln.
    #[test]
    fn test_ordinary_error_leaves_feature_locked_none() {
        let command_error = CommandError::with_code("irgendein fachlicher Fehler", "TEST_ERROR");

        assert!(command_error.feature_locked.is_none());
        assert_eq!(command_error.code, Some("TEST_ERROR"));
    }
}

#[cfg(test)]
mod connect_log_tests {
    //! Issue #51: das Schritt-Protokoll reist nur am Verbindungsfehler mit.
    use super::*;
    use ssh_manager_core::ssh::{ConnectStep, ConnectStepRecord, StepStatus};

    #[test]
    fn test_issue_51_connect_log_is_serialized_only_when_attached() {
        let plain = serde_json::to_value(CommandError::with_code("x", "SSH_TIMEOUT")).unwrap();
        assert!(plain.get("connect_log").is_none(), "{plain}");

        let step = ConnectStepRecord {
            hop_index: 0,
            hop: "deploy@example.invalid:22".into(),
            step: ConnectStep::TcpConnect {
                address: None,
                port: 22,
            },
            status: StepStatus::Failed {
                code: "SSH_TIMEOUT".into(),
            },
            duration_ms: Some(10_000),
        };
        let with_log = serde_json::to_value(
            CommandError::with_code("x", "SSH_TIMEOUT").with_connect_log(vec![step]),
        )
        .unwrap();
        assert_eq!(with_log["code"], "SSH_TIMEOUT");
        assert_eq!(with_log["connect_log"][0]["step"]["kind"], "tcpConnect");
        assert_eq!(with_log["connect_log"][0]["status"]["code"], "SSH_TIMEOUT");
    }
}

#[cfg(test)]
mod store_code_tests {
    use super::*;
    use persistence_sqlite::StoreErrorKind;

    fn code_of(err: CommandError) -> Option<&'static str> {
        let json = serde_json::to_value(&err).unwrap();
        // The serialized `code` is what the frontend sees.
        let code = json.get("code").and_then(|c| c.as_str()).map(str::to_owned);
        assert_eq!(code.as_deref(), err.code);
        err.code
    }

    #[test]
    fn test_store_error_code_maps_every_kind() {
        assert_eq!(StoreErrorKind::Busy.code(), STORE_BUSY);
        assert_eq!(StoreErrorKind::Constraint.code(), STORE_CONSTRAINT);
        assert_eq!(StoreErrorKind::Corrupt.code(), STORE_CORRUPT);
        assert_eq!(StoreErrorKind::Io.code(), STORE_IO);
        assert_eq!(StoreErrorKind::NotFound.code(), STORE_FAILED);
        assert_eq!(StoreErrorKind::Other.code(), STORE_FAILED);
    }

    #[test]
    fn test_profile_store_error_reaches_the_frontend_with_a_store_code() {
        let err = ssh_manager_core::profiles::ProfileError::Store {
            kind: StoreErrorKind::Busy,
            message: "database is locked".into(),
        };
        let cmd = CommandError::from(err);
        assert_eq!(
            cmd.message,
            "Profile-Store-Backend-Fehler: database is locked"
        );
        assert_eq!(code_of(cmd), Some(STORE_BUSY));
    }

    #[test]
    fn test_domain_errors_keep_the_plain_conversion() {
        let err = ssh_manager_core::profiles::ProfileError::CycleDetected;
        assert_eq!(code_of(CommandError::from(err)), Some("GROUP_CHAIN_CYCLE"));
    }

    #[test]
    fn test_chat_session_store_error_reaches_the_frontend_with_a_store_code() {
        let err =
            persistence_sqlite::ChatSessionStoreError::Database(persistence_sqlite::DbFailure {
                kind: StoreErrorKind::Constraint,
                message: "FOREIGN KEY constraint failed".into(),
            });
        assert_eq!(
            code_of(Err::<(), _>(err).store_err().unwrap_err()),
            Some(STORE_CONSTRAINT)
        );
    }

    #[test]
    fn test_ai_provider_store_error_reaches_the_frontend_with_a_store_code() {
        use persistence_sqlite::AiProviderStoreError;
        let err = AiProviderStoreError::Database(persistence_sqlite::DbFailure {
            kind: StoreErrorKind::Corrupt,
            message: "malformed".into(),
        });
        assert_eq!(code_of(CommandError::from(err)), Some(STORE_CORRUPT));
    }
}

#[cfg(test)]
mod every_error_has_a_code_tests {
    use super::*;
    use ssh_manager_core::ai::AiError;
    use ssh_manager_core::profiles::{ProfileError, StoreErrorKind};
    use ssh_manager_core::ssh::SshError;

    /// Spec 0024, section 5: errors converted through `?` always carry a code.
    #[test]
    fn test_converted_errors_carry_a_code() {
        let io = CommandError::from(std::io::Error::other("disk"));
        assert_eq!(io.code, Some(ssh_manager_core::ipc_error::IO_FAILED));

        let profile = CommandError::from(ProfileError::ServerNotFound(Default::default()));
        assert_eq!(profile.code, Some("SERVER_NOT_FOUND"));

        let stringly = CommandError::with_code("x", "TEST_ERROR");
        assert_eq!(stringly.code, Some("TEST_ERROR"));
    }

    /// One representative error per command module whose errors travel
    /// through `?` (the modules whose errors are built by hand set the code at
    /// the construction site and are covered in `app-shell`).
    #[test]
    fn test_representative_error_of_every_command_module_has_a_code() {
        let samples: Vec<(&str, CommandError)> = vec![
            (
                "servers",
                ProfileError::ServerNotFound(Default::default()).into(),
            ),
            ("groups", ProfileError::CycleDetected.into()),
            (
                "profile store",
                ProfileError::Store {
                    kind: StoreErrorKind::Busy,
                    message: "m".into(),
                }
                .into(),
            ),
            ("connect", SshError::ConnectionFailed("x".into()).into()),
            ("sftp", SshError::SftpPermissionDenied("x".into()).into()),
            ("elevation", SshError::AuthenticationFailed.into()),
            ("terminal", SshError::SessionClosed("x".into()).into()),
            ("chat", AiError::RateLimited.into()),
            ("ai providers", AiError::AuthenticationFailed.into()),
            (
                "filter rules",
                crate::filter_rules::RuleWriteError::Store(
                    persistence_sqlite::PolicyStoreError::Backend("x".into()),
                )
                .into(),
            ),
            (
                "filter rules (store kind)",
                persistence_sqlite::PolicyStoreError::NotFound(ssh_manager_core::filter::RuleId(
                    "r".into(),
                ))
                .into(),
            ),
            (
                "chat sessions",
                persistence_sqlite::ChatSessionStoreError::Backend("x".into()).into(),
            ),
            (
                "files (mcp settings, speech)",
                std::io::Error::other("x").into(),
            ),
        ];
        for (module, err) in samples {
            assert!(err.code.is_some(), "{module}: error without a code");
        }
    }

    /// No hand-built `CommandError { .. }` literal may leave `code` empty
    /// (that would reopen the hole the `ErrorCode` bound closed).
    #[test]
    fn test_no_command_error_literal_without_a_code() {
        for file in rust_sources() {
            let text = std::fs::read_to_string(&file).unwrap();
            for (at, _) in text.match_indices("CommandError {") {
                let head = &text[..at];
                if head.ends_with("struct ") || head.ends_with("impl ") {
                    continue;
                }
                let tail: String = text[at..].chars().take(160).collect();
                assert!(
                    !tail.contains("code: None"),
                    "{}: CommandError literal with `code: None`",
                    file.display()
                );
            }
        }
    }

    /// Every `*.rs` file under `app-logic/src` and `app-shell/src`.
    fn rust_sources() -> Vec<std::path::PathBuf> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut out = Vec::new();
        for krate in ["app-logic", "app-shell"] {
            walk(&root.join(krate).join("src"), &mut out);
        }
        out.sort();
        out
    }

    /// Value of a code-shaped `const NAME: &str = "UPPER_CASE";` line.
    fn code_const(line: &str) -> Option<(&str, &str)> {
        let rest = line.trim_start();
        let rest = rest
            .strip_prefix("pub(crate) ")
            .or_else(|| rest.strip_prefix("pub "))
            .unwrap_or(rest);
        let rest = rest.strip_prefix("const ")?;
        let (name, rest) = rest.split_once(": &str = \"")?;
        let value = rest.strip_suffix("\";")?;
        let shaped = value.len() > 3
            && value
                .chars()
                .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit());
        shaped.then_some((name, value))
    }

    /// Issue #887 AC 4: the codes the command layers define as constants in
    /// `app-logic`/`app-shell` are collected from the source and checked
    /// together with the codes of the `ErrorCode` types: one name per value,
    /// and no constant reuses a type's code.
    #[test]
    fn test_code_constants_in_the_command_layers_are_unique() {
        let mut named: Vec<(String, String)> = Vec::new();
        let mut inline: Vec<String> = Vec::new();
        for file in rust_sources() {
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            if name.starts_with("test") || file.components().any(|c| c.as_os_str() == "tests") {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            let production = text.split("#[cfg(test)]").next().unwrap();
            for (at, call) in production
                .match_indices("with_code(")
                .chain(production.match_indices("or_code("))
            {
                // The code is the last argument: the last quoted
                // UPPER_CASE literal before the call's closing `)`.
                let args: String = production[at + call.len()..].chars().take(300).collect();
                let Some(end) = args.find("\")") else {
                    continue;
                };
                let Some(lit) = args[..end].rsplit('"').next() else {
                    continue;
                };
                let shaped = lit.len() > 3
                    && lit
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit());
                if shaped {
                    inline.push(lit.to_owned());
                }
            }
            for line in production.lines() {
                if let Some((name, value)) = code_const(line) {
                    named.push((name.to_owned(), value.to_owned()));
                }
            }
        }
        assert!(
            named.len() > 20,
            "the scan found only {} constants",
            named.len()
        );
        for (i, (name, value)) in named.iter().enumerate() {
            for (other, other_value) in &named[i + 1..] {
                assert!(
                    !(value == other_value && name != other),
                    "code {value} is defined by both {name} and {other}"
                );
            }
        }
        let typed: Vec<&str> = [
            SshError::CODES,
            AiError::CODES,
            ProfileError::CODES,
            persistence_sqlite::STORE_ERROR_CODES,
        ]
        .concat();
        assert!(!inline.is_empty(), "the scan found no inline codes");
        for lit in &inline {
            assert!(
                !typed.contains(&lit.as_str()) || lit == SECRET_STORE_FAILED,
                "inline code {lit} reuses the code of an ErrorCode type"
            );
        }
        for (name, value) in &named {
            // `SECRET_STORE_FAILED` is deliberately shared with `SshError`.
            if value == SECRET_STORE_FAILED {
                continue;
            }
            assert!(
                !typed.contains(&value.as_str()),
                "constant {name} reuses the code {value} of an ErrorCode type"
            );
        }
    }

    /// The scan helper above sees a duplicate (fixture).
    #[test]
    fn test_code_const_parses_a_definition() {
        assert_eq!(
            code_const("pub const FOO_BAR: &str = \"FOO_BAR\";"),
            Some(("FOO_BAR", "FOO_BAR"))
        );
        assert_eq!(code_const("const X: &str = \"a b\";"), None);
    }
}
